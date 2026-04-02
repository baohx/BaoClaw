use std::sync::Arc;
use std::path::PathBuf;

mod api;
mod bridge;
mod discovery;
mod engine;
mod ipc;
mod mcp;
mod models;
mod permissions;
mod state;
mod tools;

use api::client::{AnthropicClient, ApiClientConfig};
use engine::query_engine::{EngineEvent, QueryEngine, QueryEngineConfig, ThinkingConfig, EMPTY_USAGE};
use ipc::events::{send_engine_event, send_state_patches};
use ipc::protocol::JsonRpcMessage;
use ipc::router::{parse_client_method, ClientMethod};
use ipc::server::{IpcError, IpcServer};
use state::manager::{CoreState, StateManager};
use tools::builtins::{BashTool, FileEditTool, FileReadTool, FileWriteTool};

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
    let cwd = PathBuf::from(&cwd_str);

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

    // Get API key and config from environment
    let api_key = std::env::var("ANTHROPIC_API_KEY").unwrap_or_default();
    let api_client = Arc::new(AnthropicClient::new(ApiClientConfig {
        api_key,
        base_url: std::env::var("ANTHROPIC_BASE_URL").ok(),
        max_retries: None,
    }));

    let engine_tools: Vec<Arc<dyn tools::Tool>> = vec![
        Arc::new(BashTool::new()),
        Arc::new(FileReadTool::new(vec![])),
        Arc::new(FileWriteTool::new(vec![])),
        Arc::new(FileEditTool::new(vec![])),
    ];

    let session_id = uuid::Uuid::new_v4().to_string();

    // Write metadata file for discovery by CLI
    write_meta(&socket_path, &cwd_str, &session_id);

    let state_manager = StateManager::new(CoreState {
        session_id: session_id.clone(),
        model: "claude-sonnet-4-20250514".to_string(),
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

        let (init_id, init_cwd, init_model) = match init_msg {
            JsonRpcMessage::Request(req) => {
                let id = req.id.clone();
                match parse_client_method(&req) {
                    Ok(ClientMethod::Initialize { cwd: c, model: m, .. }) => {
                        (id, c, m)
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

        let model = init_model.unwrap_or_else(|| "claude-sonnet-4-20250514".to_string());
        let work_cwd = init_cwd;

        // Create engine if first client, or reuse existing
        if engine.is_none() {
            engine = Some(QueryEngine::new(QueryEngineConfig {
                cwd: work_cwd.clone(),
                tools: engine_tools.clone(),
                api_client: Arc::clone(&api_client),
                model: model.clone(),
                thinking_config: ThinkingConfig::Disabled,
                max_turns: None,
                max_budget_usd: None,
                verbose: false,
                custom_system_prompt: None,
                append_system_prompt: None,
            }));
        }

        let eng = engine.as_mut().unwrap();
        let msg_count = eng.get_messages().len();

        // Send init response — include whether this is a reconnection
        let _ = conn.send_response(init_id, serde_json::json!({
            "capabilities": { "tools": true, "streaming": true, "permissions": true },
            "session_id": &session_id,
            "reconnected": msg_count > 0,
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
                                        ClientMethod::UpdateSettings { .. } => {
                                            let _ = conn.send_response(id, serde_json::json!("ok")).await;
                                        }
                                        ClientMethod::PermissionResponse { .. } => {
                                            let _ = conn.send_response(id, serde_json::json!("ok")).await;
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
