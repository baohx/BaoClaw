use std::sync::Arc;
use std::path::PathBuf;

mod api;
mod bridge;
mod config;
mod discovery;
mod engine;
mod ipc;
mod mcp;
mod models;
mod permissions;
mod state;
mod telemetry;
mod tools;
mod updater;

use api::client::{AnthropicClient, ApiClientConfig};
use engine::query_engine::{EngineEvent, QueryEngine, QueryEngineConfig, ThinkingConfig, EMPTY_USAGE};
use engine::task_manager::TaskManager;
use engine::transcript::{TranscriptWriter, rebuild_messages_from_transcript};
use ipc::events::{send_engine_event, send_state_patches};
use ipc::protocol::JsonRpcMessage;
use ipc::router::{parse_client_method, ClientMethod};
use ipc::server::{IpcError, IpcServer};
use permissions::gate::{PermissionDecision, PermissionGate};
use state::manager::{CoreState, StateManager};
use tools::builtins::{AgentTool, BashTool, FileEditTool, FileReadTool, FileWriteTool, GlobTool, GrepTool, NotebookEditTool, TodoWriteTool, ToolSearchTool, WebFetchTool, WebSearchTool};
use mcp::tool_wrapper::McpToolWrapper;

/// Socket directory for all BaoClaw daemon instances
fn socket_dir() -> PathBuf {
    let dir = std::env::temp_dir().join("baoclaw-sockets");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// Generate a socket path with session info embedded in the filename
fn make_socket_path(cwd: &str) -> PathBuf {
    let pid = std::process::id();
    // Use a hash of cwd for the filename so we can identify which dir it serves
    let cwd_hash = &format!("{:x}", md5_simple(cwd))[..8];
    socket_dir().join(format!("baoclaw-{}-{}.sock", cwd_hash, pid))
}

/// Write a metadata JSON file next to the socket for discovery
fn write_meta(socket_path: &std::path::Path, cwd: &str, session_id: &str) {
    let meta_path = socket_path.with_extension("json");
    let meta = serde_json::json!({
        "pid": std::process::id(),
        "cwd": cwd,
        "session_id": session_id,
        "socket": socket_path.to_string_lossy(),
        "started_at": chrono::Utc::now().to_rfc3339(),
    });
    let _ = std::fs::write(&meta_path, serde_json::to_string_pretty(&meta).unwrap_or_default());
}

fn cleanup_meta(socket_path: &std::path::Path) {
    let _ = std::fs::remove_file(socket_path.with_extension("json"));
}

/// Simple hash for cwd → short hex string
fn md5_simple(input: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in input.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let is_daemon = args.iter().any(|a| a == "--daemon");

    // CRITICAL: Ignore SIGPIPE so we don't die when CLI disconnects stdout/stderr
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_IGN);
    }

    // Parse --cwd flag or use current directory
    let cwd_str = args.iter().position(|a| a == "--cwd")
        .and_then(|i| args.get(i + 1))
        .map(|s| s.to_string())
        .unwrap_or_else(|| std::env::current_dir().unwrap().to_string_lossy().to_string());
    let _cwd = PathBuf::from(&cwd_str);

    // Parse --resume flag for session resumption
    let cli_resume_session_id = args.iter().position(|a| a == "--resume")
        .and_then(|i| args.get(i + 1))
        .map(|s| s.to_string());

    // Parse --think flag for extended thinking
    let cli_thinking_config = if args.iter().any(|a| a == "--think") {
        let budget = args.iter().position(|a| a == "--think")
            .and_then(|i| args.get(i + 1))
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(10240);
        ThinkingConfig::Enabled { budget_tokens: budget }
    } else {
        ThinkingConfig::Disabled
    };

    // Create socket in the shared socket directory
    let socket_path = make_socket_path(&cwd_str);

    // Bind IPC server
    let server = IpcServer::bind(&socket_path).await?;

    // Output socket path for clients to find
    println!("SOCKET:{}", socket_path.display());
    use std::io::Write;
    std::io::stdout().flush()?;

    // In daemon mode, close stdout/stderr after emitting socket path
    // so broken pipes from the launching CLI can't affect us
    if is_daemon {
        // Redirect stdout and stderr to a log file or /dev/null
        let log_path = socket_path.with_extension("log");
        let log_file = std::fs::OpenOptions::new()
            .create(true).append(true).open(&log_path).ok();

        #[cfg(unix)]
        {
            use std::os::unix::io::AsRawFd;
            // Redirect stderr to log file (or /dev/null)
            if let Some(ref f) = log_file {
                unsafe {
                    libc::dup2(f.as_raw_fd(), 2); // stderr → log
                }
            } else {
                let devnull = std::fs::File::open("/dev/null").unwrap();
                unsafe {
                    libc::dup2(devnull.as_raw_fd(), 2);
                }
            }
            // Redirect stdout to /dev/null (we already sent SOCKET: line)
            let devnull = std::fs::File::open("/dev/null").unwrap();
            unsafe {
                libc::dup2(devnull.as_raw_fd(), 1); // stdout → /dev/null
            }
        }

        eprintln!("baoclaw-core daemon started (pid={}, cwd={})", std::process::id(), cwd_str);
    }

    // Load BaoClaw config from ~/.baoclaw/config.json
    let mut baoclaw_config = config::load_config();
    config::apply_env_override(&mut baoclaw_config);

    // Get API key and config from environment
    let api_key = std::env::var("ANTHROPIC_API_KEY").unwrap_or_default();
    let api_client = Arc::new(AnthropicClient::new(ApiClientConfig {
        api_key,
        base_url: std::env::var("ANTHROPIC_BASE_URL").ok(),
        max_retries: None,
    }));

    // Read-only tool subset for sub-agent use
    let read_only_tools: Vec<Arc<dyn tools::Tool>> = vec![
        Arc::new(FileReadTool::new(vec![])),
        Arc::new(GrepTool::new()),
        Arc::new(GlobTool::new()),
        Arc::new(WebFetchTool::new()),
    ];

    let engine_tools: Vec<Arc<dyn tools::Tool>> = vec![
        Arc::new(BashTool::new()),
        Arc::new(FileReadTool::new(vec![])),
        Arc::new(FileWriteTool::new(vec![])),
        Arc::new(FileEditTool::new(vec![])),
        Arc::new(WebFetchTool::new()),
        Arc::new(WebSearchTool::new()),
        Arc::new(NotebookEditTool::new()),
        Arc::new(TodoWriteTool::new()),
        Arc::new(AgentTool::new(Arc::clone(&api_client), read_only_tools)),
    ];

    // ToolSearchTool needs the full tool list, so register it last
    let engine_tools: Vec<Arc<dyn tools::Tool>> = {
        let mut all = engine_tools;

        // MCP integration: discover and connect to MCP servers
        let mcp_servers = discovery::mcp_config::discover_mcp_servers(std::path::Path::new(&cwd_str)).await;
        for server_info in &mcp_servers {
            if server_info.disabled {
                continue;
            }
            if let Some(ref command) = server_info.command {
                let config = mcp::McpServerConfig {
                    name: server_info.name.clone(),
                    command: command.clone(),
                    args: server_info.args.clone(),
                    env: std::collections::HashMap::new(),
                    transport: mcp::McpTransportType::Stdio,
                };
                let mut client = mcp::McpClient::new(config);
                match client.connect_stdio().await {
                    Ok(()) => {
                        let client = Arc::new(client);
                        if let Ok(tools) = client.list_tools().await {
                            for tool_def in tools {
                                let wrapper = McpToolWrapper::new(
                                    Arc::clone(&client),
                                    tool_def,
                                    server_info.name.clone(),
                                );
                                all.push(Arc::new(wrapper));
                            }
                        }
                        eprintln!("MCP server '{}' connected", server_info.name);
                    }
                    Err(e) => {
                        eprintln!("Warning: MCP server '{}' failed to connect: {}", server_info.name, e);
                    }
                }
            }
        }

        all.push(Arc::new(ToolSearchTool::new(all.clone())));
        all
    };

    let session_id = uuid::Uuid::new_v4().to_string();

    // Write metadata file for discovery by CLI
    write_meta(&socket_path, &cwd_str, &session_id);

    let state_manager = StateManager::new(CoreState {
        session_id: session_id.clone(),
        model: baoclaw_config.model.clone(),
        verbose: false,
        tasks: std::collections::HashMap::new(),
        usage: EMPTY_USAGE,
        total_cost_usd: 0.0,
    });

    // If daemon mode, ignore SIGHUP so we survive terminal close
    if is_daemon {
        #[cfg(unix)]
        unsafe {
            libc::signal(libc::SIGHUP, libc::SIG_IGN);
        }
    }

    // ══════════════════════════════════════════════════════════
    // Main accept loop — keeps running, accepts new clients
    // When a client disconnects, we wait for the next one
    // Only `shutdown` RPC terminates the daemon
    // ══════════════════════════════════════════════════════════
    let mut should_exit = false;

    // We need to create the engine outside the loop so conversation persists
    let mut engine: Option<QueryEngine> = None;

    // Create PermissionGate for the permission interactive flow
    let permission_gate = PermissionGate::new();

    // Create TaskManager for background task execution
    let task_manager = Arc::new(TaskManager::new(
        Arc::clone(&api_client),
        engine_tools.clone(),
    ));

    while !should_exit {
        eprintln!("Waiting for client connection...");
        let mut conn = match server.accept().await {
            Ok(c) => c,
            Err(e) => {
                eprintln!("Accept error: {}", e);
                continue;
            }
        };
        eprintln!("Client connected");

        // Wait for initialize (or re-initialize) request
        let init_msg = match conn.recv_message().await {
            Ok(msg) => msg,
            Err(IpcError::ConnectionClosed) => {
                eprintln!("Client disconnected before initialize");
                continue;
            }
            Err(e) => {
                eprintln!("Error reading init: {}", e);
                continue;
            }
        };

        let (init_id, init_cwd, init_model, init_resume_session_id) = match init_msg {
            JsonRpcMessage::Request(req) => {
                let id = req.id.clone();
                match parse_client_method(&req) {
                    Ok(ClientMethod::Initialize { cwd: c, model: m, resume_session_id: r, .. }) => {
                        (id, c, m, r)
                    }
                    Ok(_) => {
                        let _ = conn.send_error(Some(req.id), -32600,
                            "Expected 'initialize' as first request".into()).await;
                        continue;
                    }
                    Err(e) => {
                        let _ = conn.send_error(Some(req.id), -32600,
                            format!("Invalid init: {}", e)).await;
                        continue;
                    }
                }
            }
            _ => { continue; }
        };

        let model = init_model
            .or_else(|| std::env::var("ANTHROPIC_MODEL").ok())
            .unwrap_or_else(|| baoclaw_config.model.clone());
        let work_cwd = init_cwd;

        // Create engine if first client, or reuse existing
        if engine.is_none() {
            engine = Some(QueryEngine::new(QueryEngineConfig {
                cwd: work_cwd.clone(),
                tools: engine_tools.clone(),
                api_client: Arc::clone(&api_client),
                model: model.clone(),
                thinking_config: cli_thinking_config.clone(),
                max_turns: None,
                max_budget_usd: None,
                verbose: false,
                custom_system_prompt: None,
                append_system_prompt: None,
                session_id: Some(session_id.clone()),
                fallback_models: baoclaw_config.fallback_models.clone(),
                max_retries_per_model: baoclaw_config.max_retries_per_model,
            }));
        }

        let eng = engine.as_mut().unwrap();

        // Handle session resume if requested (from RPC or CLI arg)
        let resume_id = init_resume_session_id.or(cli_resume_session_id.clone());
        let mut resumed = false;
        if let Some(ref resume_id) = resume_id {
            match TranscriptWriter::load(resume_id) {
                Ok(entries) => {
                    let messages = rebuild_messages_from_transcript(&entries);
                    if !messages.is_empty() {
                        eng.set_messages(messages);
                        resumed = true;
                        eprintln!("Resumed session {} ({} messages)", resume_id, eng.get_messages().len());
                    }
                }
                Err(e) => {
                    eprintln!("Failed to load transcript for session {}: {}", resume_id, e);
                }
            }
        }

        let msg_count = eng.get_messages().len();

        // Send init response — include whether this is a reconnection
        let _ = conn.send_response(init_id, serde_json::json!({
            "capabilities": { "tools": true, "streaming": true, "permissions": true },
            "session_id": &session_id,
            "reconnected": msg_count > 0,
            "resumed": resumed,
            "message_count": msg_count,
        })).await;

        // State patch forwarding
        let (patch_tx, mut patch_rx) =
            tokio::sync::mpsc::channel::<Vec<state::manager::StatePatch>>(256);
        {
            let mut state_rx = state_manager.subscribe();
            let ptx = patch_tx.clone();
            tokio::spawn(async move {
                loop {
                    match state_rx.recv().await {
                        Ok(patches) => { if ptx.send(patches).await.is_err() { break; } }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    }
                }
            });
        }

        // ── Client RPC loop ──
        let mut client_disconnected = false;
        while !client_disconnected && !should_exit {
            tokio::select! {
                Some(patches) = patch_rx.recv() => {
                    if let Err(_) = send_state_patches(&mut conn, &patches).await {
                        client_disconnected = true;
                    }
                }
                msg_result = conn.recv_message() => {
                    let msg = match msg_result {
                        Ok(msg) => msg,
                        Err(IpcError::ConnectionClosed) => {
                            eprintln!("Client disconnected");
                            client_disconnected = true;
                            continue;
                        }
                        Err(e) => {
                            eprintln!("IPC error: {}", e);
                            client_disconnected = true;
                            continue;
                        }
                    };

                    match msg {
                        JsonRpcMessage::Request(req) => {
                            let id = req.id.clone();
                            match parse_client_method(&req) {
                                Ok(method) => {
                                    let eng = engine.as_mut().unwrap();
                                    match method {
                                        ClientMethod::SubmitMessage { prompt, .. } => {
                                            let prompt_str = match prompt.as_str() {
                                                Some(s) => s.to_string(),
                                                None => serde_json::to_string(&prompt).unwrap_or_default(),
                                            };
                                            let mut rx = eng.submit_message(prompt_str).await;
                                            while let Some(event) = rx.recv().await {
                                                if send_engine_event(&mut conn, &event).await.is_err() {
                                                    client_disconnected = true;
                                                    break;
                                                }
                                                if matches!(event, EngineEvent::Result(_) | EngineEvent::Error(_)) {
                                                    break;
                                                }
                                            }
                                            if !client_disconnected {
                                                // Sync messages back from the spawned query loop
                                                eng.sync_messages().await;
                                                let _ = conn.send_response(id, serde_json::json!({"status": "complete"})).await;
                                            }
                                        }
                                        ClientMethod::Abort => {
                                            eng.abort();
                                            let _ = conn.send_response(id, serde_json::json!("ok")).await;
                                        }
                                        ClientMethod::Shutdown => {
                                            let _ = conn.send_response(id, serde_json::json!("ok")).await;
                                            should_exit = true;
                                        }
                                        // /disconnect — client leaves, daemon stays
                                        ClientMethod::UpdateSettings { settings } => {
                                            // Handle thinking config updates
                                            if let Some(thinking) = settings.get("thinking") {
                                                if let Some(mode) = thinking.get("mode").and_then(|v| v.as_str()) {
                                                    let eng = engine.as_mut().unwrap();
                                                    match mode {
                                                        "enabled" => {
                                                            let budget = thinking.get("budget_tokens")
                                                                .and_then(|v| v.as_u64())
                                                                .unwrap_or(10240) as u32;
                                                            eng.update_thinking_config(ThinkingConfig::Enabled { budget_tokens: budget });
                                                        }
                                                        "adaptive" => {
                                                            eng.update_thinking_config(ThinkingConfig::Adaptive);
                                                        }
                                                        _ => {
                                                            eng.update_thinking_config(ThinkingConfig::Disabled);
                                                        }
                                                    }
                                                }
                                            }
                                            let _ = conn.send_response(id, serde_json::json!("ok")).await;
                                        }
                                        ClientMethod::PermissionResponse { tool_use_id, decision, rule } => {
                                            let perm_decision = match decision.as_str() {
                                                "allow" => PermissionDecision::Allow,
                                                "allow_always" => PermissionDecision::AllowAlways { rule },
                                                _ => PermissionDecision::Deny,
                                            };
                                            let delivered = permission_gate.respond(&tool_use_id, perm_decision);
                                            let _ = conn.send_response(id, serde_json::json!({"delivered": delivered})).await;
                                        }
                                        ClientMethod::Initialize { .. } => {
                                            let _ = conn.send_error(Some(id), -32600, "Already initialized".into()).await;
                                        }
                                        ClientMethod::ListTools => {
                                            let tl: Vec<serde_json::Value> = engine_tools.iter().map(|t| {
                                                serde_json::json!({"name": t.name(), "description": t.prompt(), "type": "builtin"})
                                            }).collect();
                                            let _ = conn.send_response(id, serde_json::json!({"tools": tl, "count": tl.len()})).await;
                                        }
                                        ClientMethod::ListMcpServers => {
                                            let s = discovery::mcp_config::discover_mcp_servers(&work_cwd).await;
                                            let _ = conn.send_response(id, serde_json::json!({"servers": s, "count": s.len()})).await;
                                        }
                                        ClientMethod::ListSkills => {
                                            let s = discovery::skills::discover_skills(&work_cwd).await;
                                            let _ = conn.send_response(id, serde_json::json!({"skills": s, "count": s.len()})).await;
                                        }
                                        ClientMethod::ListPlugins => {
                                            let p = discovery::plugins::discover_plugins(&work_cwd).await;
                                            let _ = conn.send_response(id, serde_json::json!({"plugins": p, "count": p.len()})).await;
                                        }
                                        ClientMethod::Compact => {
                                            let eng = engine.as_mut().unwrap();
                                            match eng.compact().await {
                                                Ok(result) => {
                                                    let _ = conn.send_response(id, serde_json::json!({
                                                        "tokens_saved": result.tokens_saved,
                                                        "summary_tokens": result.summary_tokens,
                                                    })).await;
                                                }
                                                Err(e) => {
                                                    let _ = conn.send_error(Some(id), -32000, e.message).await;
                                                }
                                            }
                                        }
                                        ClientMethod::SwitchModel { model: new_model } => {
                                            let eng = engine.as_mut().unwrap();
                                            eng.update_model(new_model.clone());
                                            state_manager.update(|s| { s.model = new_model.clone(); });
                                            let _ = conn.send_response(id, serde_json::json!({"model": new_model})).await;
                                        }
                                        ClientMethod::GitDiff => {
                                            let output = tokio::process::Command::new("git")
                                                .args(["diff", "--stat"])
                                                .current_dir(&work_cwd)
                                                .output()
                                                .await;
                                            match output {
                                                Ok(o) if o.status.success() => {
                                                    let stdout = String::from_utf8_lossy(&o.stdout).to_string();
                                                    let result = if stdout.trim().is_empty() {
                                                        "No uncommitted changes.".to_string()
                                                    } else {
                                                        stdout
                                                    };
                                                    let _ = conn.send_response(id, serde_json::json!({"diff": result})).await;
                                                }
                                                Ok(o) => {
                                                    let stderr = String::from_utf8_lossy(&o.stderr).to_string();
                                                    let _ = conn.send_error(Some(id), -32000, format!("git diff failed: {}", stderr)).await;
                                                }
                                                Err(e) => {
                                                    let _ = conn.send_error(Some(id), -32000, format!("Not a git repository or git not available: {}", e)).await;
                                                }
                                            }
                                        }
                                        ClientMethod::GitCommit { message } => {
                                            // Stage all changes
                                            let add_result = tokio::process::Command::new("git")
                                                .args(["add", "-A"])
                                                .current_dir(&work_cwd)
                                                .output()
                                                .await;
                                            match add_result {
                                                Ok(o) if o.status.success() => {
                                                    // Commit
                                                    let commit_result = tokio::process::Command::new("git")
                                                        .args(["commit", "-m", &message])
                                                        .current_dir(&work_cwd)
                                                        .output()
                                                        .await;
                                                    match commit_result {
                                                        Ok(co) if co.status.success() => {
                                                            // Get commit hash
                                                            let hash = tokio::process::Command::new("git")
                                                                .args(["rev-parse", "--short", "HEAD"])
                                                                .current_dir(&work_cwd)
                                                                .output()
                                                                .await
                                                                .ok()
                                                                .and_then(|h| String::from_utf8(h.stdout).ok())
                                                                .map(|s| s.trim().to_string())
                                                                .unwrap_or_default();
                                                            let _ = conn.send_response(id, serde_json::json!({"hash": hash, "message": message})).await;
                                                        }
                                                        Ok(co) => {
                                                            let stderr = String::from_utf8_lossy(&co.stderr).to_string();
                                                            let stdout = String::from_utf8_lossy(&co.stdout).to_string();
                                                            let msg = if stderr.is_empty() { stdout } else { stderr };
                                                            let _ = conn.send_error(Some(id), -32000, format!("git commit failed: {}", msg)).await;
                                                        }
                                                        Err(e) => {
                                                            let _ = conn.send_error(Some(id), -32000, format!("git commit error: {}", e)).await;
                                                        }
                                                    }
                                                }
                                                Ok(o) => {
                                                    let stderr = String::from_utf8_lossy(&o.stderr).to_string();
                                                    let _ = conn.send_error(Some(id), -32000, format!("git add failed: {}", stderr)).await;
                                                }
                                                Err(e) => {
                                                    let _ = conn.send_error(Some(id), -32000, format!("Not a git repository or git not available: {}", e)).await;
                                                }
                                            }
                                        }
                                        ClientMethod::GitStatus => {
                                            let git_info = engine::git_info::get_git_info(std::path::Path::new(&work_cwd));
                                            match git_info {
                                                Some(info) => {
                                                    let _ = conn.send_response(id, serde_json::json!({
                                                        "branch": info.branch,
                                                        "has_changes": info.has_changes,
                                                        "staged_files": info.staged_files,
                                                        "modified_files": info.modified_files,
                                                        "untracked_files": info.untracked_files,
                                                    })).await;
                                                }
                                                None => {
                                                    let _ = conn.send_error(Some(id), -32000, "Not a git repository".to_string()).await;
                                                }
                                            }
                                        }
                                        ClientMethod::ListMcpResources => {
                                            // Stub: MCP resource listing requires connected clients
                                            let _ = conn.send_response(id, serde_json::json!({"resources": [], "count": 0})).await;
                                        }
                                        ClientMethod::ReadMcpResource { server_name, uri } => {
                                            let _ = conn.send_error(Some(id), -32000,
                                                format!("MCP resource read not yet wired: {}:{}", server_name, uri)).await;
                                        }
                                        ClientMethod::TaskCreate { description, prompt } => {
                                            let task_id = task_manager.create_task(
                                                description,
                                                prompt,
                                                std::path::PathBuf::from(&work_cwd),
                                                state_manager.get().model,
                                            ).await;
                                            let _ = conn.send_response(id, serde_json::json!({"task_id": task_id})).await;
                                        }
                                        ClientMethod::TaskList => {
                                            let tasks = task_manager.list_tasks().await;
                                            let _ = conn.send_response(id, serde_json::json!({"tasks": tasks, "count": tasks.len()})).await;
                                        }
                                        ClientMethod::TaskStatus { task_id } => {
                                            match task_manager.get_task_status(&task_id).await {
                                                Some(task) => {
                                                    let _ = conn.send_response(id, serde_json::json!(task)).await;
                                                }
                                                None => {
                                                    let _ = conn.send_error(Some(id), -32000,
                                                        format!("Task not found: {}", task_id)).await;
                                                }
                                            }
                                        }
                                        ClientMethod::TaskStop { task_id } => {
                                            let stopped = task_manager.stop_task(&task_id).await;
                                            let _ = conn.send_response(id, serde_json::json!({"stopped": stopped})).await;
                                        }
                                    }
                                }
                                Err(e) => {
                                    let _ = conn.send_error(Some(id), -32601, format!("{}", e)).await;
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        if !should_exit {
            eprintln!("Client session ended, waiting for next client...");
        }
    }

    // Cleanup
    cleanup_meta(&socket_path);
    drop(server);
    eprintln!("baoclaw-core shutdown complete");
    Ok(())
}
