use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::path::PathBuf;
use tokio::sync::Mutex as TokioMutex;

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

use api::client::ApiClientConfig;
use api::unified::UnifiedClient;
use config::BaoclawConfig;
use engine::query_engine::{EngineEvent, QueryEngine, QueryEngineConfig, ThinkingConfig, EMPTY_USAGE};
use engine::shared_session::{SessionRegistry, SharedSession, ClientId};
use engine::task_manager::TaskManager;
use engine::transcript::{TranscriptWriter, rebuild_messages_from_transcript};
use ipc::events::{send_engine_event, engine_event_to_notification};
use ipc::protocol::JsonRpcMessage;
use ipc::router::{parse_client_method, ClientMethod};
use ipc::server::{IpcConnection, IpcError, IpcServer};
use permissions::gate::{PermissionDecision, PermissionGate};
use state::manager::{CoreState, StateManager};
use tools::builtins::{AgentTool, BashTool, FileEditTool, FileReadTool, FileWriteTool, GlobTool, GrepTool, MemoryTool, NotebookEditTool, ProjectNoteTool, TodoWriteTool, ToolSearchTool, WebFetchTool, WebSearchTool};
use mcp::tool_wrapper::McpToolWrapper;

/// Shared state cloned into each spawned client task.
#[derive(Clone)]
struct SharedState {
    engine_tools: Vec<Arc<dyn tools::Tool>>,
    api_client: Arc<UnifiedClient>,
    permission_gate: PermissionGate,
    task_manager: Arc<TaskManager>,
    state_manager: Arc<StateManager>,
    baoclaw_config: BaoclawConfig,
    cli_thinking_config: ThinkingConfig,
    cli_resume_session_id: Option<String>,
    session_id: String,
    should_exit: Arc<AtomicBool>,
    session_registry: Arc<SessionRegistry>,
    skill_prompt: Option<String>,
    memory_store: Arc<engine::memory::MemoryStore>,
}

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

/// Build the combined append_system_prompt from skills + memory.
async fn build_append_prompt(shared: &SharedState) -> Option<String> {
    let mut parts = Vec::new();
    if let Some(ref sp) = shared.skill_prompt {
        parts.push(sp.clone());
    }
    if let Some(mp) = shared.memory_store.build_prompt_fragment().await {
        parts.push(mp);
    }
    if parts.is_empty() { None } else { Some(parts.join("\n\n")) }
}

/// Handle a client in shared mode. The client shares a QueryEngine with other clients
/// via the SharedSession. Uses ActiveSubmitter lock for concurrency control and
/// broadcast channel for event distribution.
async fn handle_shared_client(
    conn: IpcConnection,
    shared: SharedState,
    session: Arc<SharedSession>,
    client_id: ClientId,
    broadcast_rx: tokio::sync::broadcast::Receiver<EngineEvent>,
    work_cwd: PathBuf,
    _session_id: String,
) {
    // Wrap conn in Arc<TokioMutex> so the broadcast receiver task can also send
    let conn = Arc::new(TokioMutex::new(conn));

    // Spawn background task to forward broadcast events to this client (Task 5.2)
    let conn_for_broadcast = Arc::clone(&conn);
    let session_for_broadcast = session.clone();
    let broadcast_handle = tokio::spawn(async move {
        let mut rx = broadcast_rx;
        loop {
            match rx.recv().await {
                Ok(event) => {
                    // Skip forwarding if this client is the active submitter
                    // (submitter sends events directly in the submit loop)
                    if session_for_broadcast.is_active_submitter(client_id).await {
                        continue;
                    }
                    let notif = engine_event_to_notification(&event);
                    let params = serde_json::to_value(&notif.params).unwrap_or(serde_json::Value::Null);
                    let mut conn_guard = conn_for_broadcast.lock().await;
                    if conn_guard.send_notification(&notif.method, params).await.is_err() {
                        break; // Client disconnected
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    eprintln!("Shared client {} lagged by {} events", client_id, n);
                    continue;
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    break;
                }
            }
        }
    });

    // ── Shared-mode RPC loop ──
    loop {
        if shared.should_exit.load(Ordering::Relaxed) {
            break;
        }

        let msg = {
            let mut conn_guard = conn.lock().await;
            match conn_guard.recv_message().await {
                Ok(msg) => msg,
                Err(IpcError::ConnectionClosed) => {
                    eprintln!("Shared client {} disconnected", client_id);
                    break;
                }
                Err(e) => {
                    eprintln!("Shared client {} IPC error: {}", client_id, e);
                    break;
                }
            }
        };

        match msg {
            JsonRpcMessage::Request(req) => {
                let id = req.id.clone();
                match parse_client_method(&req) {
                    Ok(method) => {
                        match method {
                            // ── Task 5.1: submitMessage in shared mode ──
                            ClientMethod::SubmitMessage { prompt, attachments, .. } => {
                                if !session.try_acquire_submitter(client_id).await {
                                    let mut conn_guard = conn.lock().await;
                                    let _ = conn_guard.send_error(Some(id), -32001,
                                        "session busy: another client is currently submitting a message".into()).await;
                                    continue;
                                }

                                let prompt_str = match prompt.as_str() {
                                    Some(s) => s.to_string(),
                                    None => serde_json::to_string(&prompt).unwrap_or_default(),
                                };

                                let mut rx = {
                                    let mut engine = session.engine_write().await;
                                    engine.submit_message_with_attachments(prompt_str, attachments).await
                                };

                                let mut disconnected = false;
                                while let Some(event) = rx.recv().await {
                                    // Broadcast to all clients
                                    session.broadcast(event.clone());

                                    // Also send directly to the submitting client
                                    {
                                        let mut conn_guard = conn.lock().await;
                                        if send_engine_event(&mut *conn_guard, &event).await.is_err() {
                                            disconnected = true;
                                            break;
                                        }
                                    }

                                    if matches!(event, EngineEvent::Result(_) | EngineEvent::Error(_)) {
                                        session.release_submitter(client_id).await;
                                        let mut engine = session.engine_write().await;
                                        engine.sync_messages().await;
                                        break;
                                    }
                                }

                                if disconnected {
                                    session.release_submitter(client_id).await;
                                    break;
                                }

                                let mut conn_guard = conn.lock().await;
                                let _ = conn_guard.send_response(id, serde_json::json!({"status": "complete"})).await;
                            }

                            // ── Task 5.3: abort — any client can call ──
                            ClientMethod::Abort => {
                                let engine = session.engine_read().await;
                                engine.abort();
                                let mut conn_guard = conn.lock().await;
                                let _ = conn_guard.send_response(id, serde_json::json!("ok")).await;
                            }

                            // ── Task 6.2: shutdown in shared mode ──
                            ClientMethod::Shutdown => {
                                let mut conn_guard = conn.lock().await;
                                let _ = conn_guard.send_response(id, serde_json::json!("ok")).await;
                                // Shutdown terminates the daemon for all clients
                                shared.should_exit.store(true, Ordering::Relaxed);
                                break;
                            }

                            ClientMethod::UpdateSettings { settings } => {
                                if let Some(thinking) = settings.get("thinking") {
                                    if let Some(mode) = thinking.get("mode").and_then(|v| v.as_str()) {
                                        let mut engine = session.engine_write().await;
                                        match mode {
                                            "enabled" => {
                                                let budget = thinking.get("budget_tokens")
                                                    .and_then(|v| v.as_u64())
                                                    .unwrap_or(10240) as u32;
                                                engine.update_thinking_config(ThinkingConfig::Enabled { budget_tokens: budget });
                                            }
                                            "adaptive" => {
                                                engine.update_thinking_config(ThinkingConfig::Adaptive);
                                            }
                                            _ => {
                                                engine.update_thinking_config(ThinkingConfig::Disabled);
                                            }
                                        }
                                    }
                                }
                                let mut conn_guard = conn.lock().await;
                                let _ = conn_guard.send_response(id, serde_json::json!("ok")).await;
                            }

                            ClientMethod::PermissionResponse { tool_use_id, decision, rule } => {
                                let perm_decision = match decision.as_str() {
                                    "allow" => PermissionDecision::Allow,
                                    "allow_always" => PermissionDecision::AllowAlways { rule },
                                    _ => PermissionDecision::Deny,
                                };
                                let delivered = shared.permission_gate.respond(&tool_use_id, perm_decision);
                                let mut conn_guard = conn.lock().await;
                                let _ = conn_guard.send_response(id, serde_json::json!({"delivered": delivered})).await;
                            }

                            ClientMethod::Initialize { .. } => {
                                let mut conn_guard = conn.lock().await;
                                let _ = conn_guard.send_error(Some(id), -32600, "Already initialized".into()).await;
                            }

                            // ── Task 5.3: Read-only operations — always allowed ──
                            ClientMethod::ListTools => {
                                let tl: Vec<serde_json::Value> = shared.engine_tools.iter().map(|t| {
                                    serde_json::json!({"name": t.name(), "description": t.prompt(), "type": "builtin"})
                                }).collect();
                                let mut conn_guard = conn.lock().await;
                                let _ = conn_guard.send_response(id, serde_json::json!({"tools": tl, "count": tl.len()})).await;
                            }
                            ClientMethod::ListMcpServers => {
                                let s = discovery::mcp_config::discover_mcp_servers(&work_cwd).await;
                                let mut conn_guard = conn.lock().await;
                                let _ = conn_guard.send_response(id, serde_json::json!({"servers": s, "count": s.len()})).await;
                            }
                            ClientMethod::ListSkills => {
                                let s = discovery::skills::discover_skills(&work_cwd).await;
                                let mut conn_guard = conn.lock().await;
                                let _ = conn_guard.send_response(id, serde_json::json!({"skills": s, "count": s.len()})).await;
                            }
                            ClientMethod::ListPlugins => {
                                let p = discovery::plugins::discover_plugins(&work_cwd).await;
                                let mut conn_guard = conn.lock().await;
                                let _ = conn_guard.send_response(id, serde_json::json!({"plugins": p, "count": p.len()})).await;
                            }
                            ClientMethod::GitStatus => {
                                let git_info = engine::git_info::get_git_info(std::path::Path::new(&work_cwd));
                                let mut conn_guard = conn.lock().await;
                                match git_info {
                                    Some(info) => {
                                        let _ = conn_guard.send_response(id, serde_json::json!({
                                            "branch": info.branch,
                                            "has_changes": info.has_changes,
                                            "staged_files": info.staged_files,
                                            "modified_files": info.modified_files,
                                            "untracked_files": info.untracked_files,
                                        })).await;
                                    }
                                    None => {
                                        let _ = conn_guard.send_error(Some(id), -32000, "Not a git repository".to_string()).await;
                                    }
                                }
                            }
                            ClientMethod::GitDiff => {
                                let output = tokio::process::Command::new("git")
                                    .args(["diff", "--stat"])
                                    .current_dir(&work_cwd)
                                    .output()
                                    .await;
                                let mut conn_guard = conn.lock().await;
                                match output {
                                    Ok(o) if o.status.success() => {
                                        let stdout = String::from_utf8_lossy(&o.stdout).to_string();
                                        let result = if stdout.trim().is_empty() {
                                            "No uncommitted changes.".to_string()
                                        } else {
                                            stdout
                                        };
                                        let _ = conn_guard.send_response(id, serde_json::json!({"diff": result})).await;
                                    }
                                    Ok(o) => {
                                        let stderr = String::from_utf8_lossy(&o.stderr).to_string();
                                        let _ = conn_guard.send_error(Some(id), -32000, format!("git diff failed: {}", stderr)).await;
                                    }
                                    Err(e) => {
                                        let _ = conn_guard.send_error(Some(id), -32000, format!("Not a git repository or git not available: {}", e)).await;
                                    }
                                }
                            }

                            // ── Task 5.3: Write operations — blocked if ActiveSubmitter exists ──
                            ClientMethod::Compact => {
                                if session.has_active_submitter().await {
                                    let mut conn_guard = conn.lock().await;
                                    let _ = conn_guard.send_error(Some(id), -32002,
                                        "session busy: cannot compact while a message is being processed".into()).await;
                                    continue;
                                }
                                let mut engine = session.engine_write().await;
                                match engine.compact().await {
                                    Ok(result) => {
                                        let mut conn_guard = conn.lock().await;
                                        let _ = conn_guard.send_response(id, serde_json::json!({
                                            "tokens_saved": result.tokens_saved,
                                            "summary_tokens": result.summary_tokens,
                                            "tokens_before": result.tokens_before,
                                            "tokens_after": result.tokens_after,
                                        })).await;
                                    }
                                    Err(e) => {
                                        let mut conn_guard = conn.lock().await;
                                        let _ = conn_guard.send_error(Some(id), -32000, e.message).await;
                                    }
                                }
                            }
                            ClientMethod::SwitchModel { model: new_model } => {
                                if session.has_active_submitter().await {
                                    let mut conn_guard = conn.lock().await;
                                    let _ = conn_guard.send_error(Some(id), -32002,
                                        "session busy: cannot switch model while a message is being processed".into()).await;
                                    continue;
                                }
                                let mut engine = session.engine_write().await;
                                engine.update_model(new_model.clone());
                                shared.state_manager.update(|s| { s.model = new_model.clone(); });
                                let mut conn_guard = conn.lock().await;
                                let _ = conn_guard.send_response(id, serde_json::json!({"model": new_model})).await;
                            }

                            ClientMethod::SwitchCwd { cwd: new_cwd } => {
                                if session.has_active_submitter().await {
                                    let mut conn_guard = conn.lock().await;
                                    let _ = conn_guard.send_error(Some(id), -32002,
                                        "session busy: cannot switch cwd while a message is being processed".into()).await;
                                    continue;
                                }
                                let abs_cwd = if new_cwd.is_absolute() {
                                    new_cwd
                                } else {
                                    std::path::PathBuf::from(&work_cwd).join(&new_cwd)
                                };
                                if !abs_cwd.is_dir() {
                                    let mut conn_guard = conn.lock().await;
                                    let _ = conn_guard.send_error(Some(id), -32000,
                                        format!("Directory does not exist: {}", abs_cwd.display())).await;
                                } else {
                                    let baoclaw_dir = abs_cwd.join(".baoclaw");
                                    let created = if !baoclaw_dir.exists() {
                                        let _ = std::fs::create_dir_all(&baoclaw_dir);
                                        let _ = std::fs::write(baoclaw_dir.join("BAOCLAW.md"), "# Project Instructions\n\n");
                                        let _ = std::fs::write(baoclaw_dir.join("mcp.json"), "{\"mcpServers\":{}}\n");
                                        let _ = std::fs::create_dir_all(baoclaw_dir.join("skills"));
                                        true
                                    } else {
                                        false
                                    };
                                    let mut engine = session.engine_write().await;
                                    engine.update_cwd(abs_cwd.clone());
                                    // Try to resume latest session for the new project
                                    let new_cwd_str = abs_cwd.to_string_lossy().to_string();
                                    if let Some(prev_session) = engine::transcript::find_latest_session_for_cwd(&new_cwd_str) {
                                        match TranscriptWriter::load(&prev_session) {
                                            Ok(entries) => {
                                                let messages = rebuild_messages_from_transcript(&entries);
                                                if !messages.is_empty() {
                                                    engine.set_messages(messages);
                                                    eprintln!("Resumed project session {} ({} messages)", prev_session, engine.get_messages().len());
                                                } else {
                                                    engine.set_messages(vec![]);
                                                }
                                            }
                                            Err(_) => { engine.set_messages(vec![]); }
                                        }
                                    } else {
                                        engine.set_messages(vec![]); // no previous session
                                    }
                                    drop(engine); // release write lock before memory switch
                                    shared.memory_store.switch_project(&abs_cwd).await;
                                    let mut conn_guard = conn.lock().await;
                                    let _ = conn_guard.send_response(id, serde_json::json!({
                                        "cwd": abs_cwd.display().to_string(),
                                        "scaffold_created": created
                                    })).await;
                                }
                            }

                            ClientMethod::GitCommit { message } => {
                                let add_result = tokio::process::Command::new("git")
                                    .args(["add", "-A"])
                                    .current_dir(&work_cwd)
                                    .output()
                                    .await;
                                let mut conn_guard = conn.lock().await;
                                match add_result {
                                    Ok(o) if o.status.success() => {
                                        let commit_result = tokio::process::Command::new("git")
                                            .args(["commit", "-m", &message])
                                            .current_dir(&work_cwd)
                                            .output()
                                            .await;
                                        match commit_result {
                                            Ok(co) if co.status.success() => {
                                                let hash = tokio::process::Command::new("git")
                                                    .args(["rev-parse", "--short", "HEAD"])
                                                    .current_dir(&work_cwd)
                                                    .output()
                                                    .await
                                                    .ok()
                                                    .and_then(|h| String::from_utf8(h.stdout).ok())
                                                    .map(|s| s.trim().to_string())
                                                    .unwrap_or_default();
                                                let _ = conn_guard.send_response(id, serde_json::json!({"hash": hash, "message": message})).await;
                                            }
                                            Ok(co) => {
                                                let stderr = String::from_utf8_lossy(&co.stderr).to_string();
                                                let stdout = String::from_utf8_lossy(&co.stdout).to_string();
                                                let msg = if stderr.is_empty() { stdout } else { stderr };
                                                let _ = conn_guard.send_error(Some(id), -32000, format!("git commit failed: {}", msg)).await;
                                            }
                                            Err(e) => {
                                                let _ = conn_guard.send_error(Some(id), -32000, format!("git commit error: {}", e)).await;
                                            }
                                        }
                                    }
                                    Ok(o) => {
                                        let stderr = String::from_utf8_lossy(&o.stderr).to_string();
                                        let _ = conn_guard.send_error(Some(id), -32000, format!("git add failed: {}", stderr)).await;
                                    }
                                    Err(e) => {
                                        let _ = conn_guard.send_error(Some(id), -32000, format!("Not a git repository or git not available: {}", e)).await;
                                    }
                                }
                            }

                            ClientMethod::ListMcpResources => {
                                let mut conn_guard = conn.lock().await;
                                let _ = conn_guard.send_response(id, serde_json::json!({"resources": [], "count": 0})).await;
                            }
                            ClientMethod::ReadMcpResource { server_name, uri } => {
                                let mut conn_guard = conn.lock().await;
                                let _ = conn_guard.send_error(Some(id), -32000,
                                    format!("MCP resource read not yet wired: {}:{}", server_name, uri)).await;
                            }
                            ClientMethod::TaskCreate { description, prompt } => {
                                let task_id = shared.task_manager.create_task(
                                    description,
                                    prompt,
                                    std::path::PathBuf::from(&work_cwd),
                                    shared.state_manager.get().model,
                                ).await;
                                let mut conn_guard = conn.lock().await;
                                let _ = conn_guard.send_response(id, serde_json::json!({"task_id": task_id})).await;
                            }
                            ClientMethod::TaskList => {
                                let tasks = shared.task_manager.list_tasks().await;
                                let mut conn_guard = conn.lock().await;
                                let _ = conn_guard.send_response(id, serde_json::json!({"tasks": tasks, "count": tasks.len()})).await;
                            }
                            ClientMethod::TaskStatus { task_id } => {
                                match shared.task_manager.get_task_status(&task_id).await {
                                    Some(task) => {
                                        let mut conn_guard = conn.lock().await;
                                        let _ = conn_guard.send_response(id, serde_json::json!(task)).await;
                                    }
                                    None => {
                                        let mut conn_guard = conn.lock().await;
                                        let _ = conn_guard.send_error(Some(id), -32000,
                                            format!("Task not found: {}", task_id)).await;
                                    }
                                }
                            }
                            ClientMethod::TaskStop { task_id } => {
                                let stopped = shared.task_manager.stop_task(&task_id).await;
                                let mut conn_guard = conn.lock().await;
                                let _ = conn_guard.send_response(id, serde_json::json!({"stopped": stopped})).await;
                            }
                            ClientMethod::MemoryList => {
                                let entries = shared.memory_store.list().await;
                                let mut conn_guard = conn.lock().await;
                                let _ = conn_guard.send_response(id, serde_json::json!({"memories": entries, "count": entries.len()})).await;
                            }
                            ClientMethod::MemoryAdd { content, category } => {
                                let cat = engine::memory::parse_category(&category);
                                let entry = shared.memory_store.add(content, cat, "user".to_string()).await;
                                let mut conn_guard = conn.lock().await;
                                let _ = conn_guard.send_response(id, serde_json::json!({"memory": entry})).await;
                            }
                            ClientMethod::MemoryDelete { id: mem_id } => {
                                let deleted = shared.memory_store.delete(&mem_id).await;
                                let mut conn_guard = conn.lock().await;
                                let _ = conn_guard.send_response(id, serde_json::json!({"deleted": deleted})).await;
                            }
                            ClientMethod::MemoryClear => {
                                let count = shared.memory_store.clear().await;
                                let mut conn_guard = conn.lock().await;
                                let _ = conn_guard.send_response(id, serde_json::json!({"cleared": count})).await;
                            }
                        }
                    }
                    Err(e) => {
                        let mut conn_guard = conn.lock().await;
                        let _ = conn_guard.send_error(Some(id), -32601, format!("{}", e)).await;
                    }
                }
            }
            _ => {}
        }
    }

    // Cancel the broadcast receiver task
    broadcast_handle.abort();
}

/// Handle a single client connection. Each client gets its own QueryEngine
/// with independent conversation history.
async fn handle_client(mut conn: IpcConnection, shared: SharedState) {
    // Wait for initialize request
    let init_msg = match conn.recv_message().await {
        Ok(msg) => msg,
        Err(IpcError::ConnectionClosed) => {
            eprintln!("Client disconnected before initialize");
            return;
        }
        Err(e) => {
            eprintln!("Error reading init: {}", e);
            return;
        }
    };

    let (init_id, init_cwd, init_model, init_resume_session_id, init_shared_session_id) = match init_msg {
        JsonRpcMessage::Request(req) => {
            let id = req.id.clone();
            match parse_client_method(&req) {
                Ok(ClientMethod::Initialize { cwd: c, model: m, resume_session_id: r, shared_session_id: s, .. }) => {
                    (id, c, m, r, s)
                }
                Ok(_) => {
                    let _ = conn.send_error(Some(req.id), -32600,
                        "Expected 'initialize' as first request".into()).await;
                    return;
                }
                Err(e) => {
                    let _ = conn.send_error(Some(req.id), -32600,
                        format!("Invalid init: {}", e)).await;
                    return;
                }
            }
        }
        _ => { return; }
    };

    let model = init_model
        .or_else(|| std::env::var("ANTHROPIC_MODEL").ok())
        .unwrap_or_else(|| shared.baoclaw_config.model.clone());
    let mut work_cwd = init_cwd;

    // ── Shared mode branch ──
    if let Some(ref shared_session_id) = init_shared_session_id {
        let session_id_clone = shared_session_id.clone();
        let shared_clone = shared.clone();
        let model_clone = model.clone();
        let work_cwd_clone = work_cwd.clone();

        let (session, _is_new) = shared.session_registry.get_or_create(
            &session_id_clone,
            || {
                QueryEngine::new(QueryEngineConfig {
                    cwd: work_cwd_clone,
                    tools: shared_clone.engine_tools.clone(),
                    api_client: Arc::clone(&shared_clone.api_client),
                    model: model_clone,
                    thinking_config: shared_clone.cli_thinking_config.clone(),
                    max_turns: None,
                    max_budget_usd: None,
                    verbose: false,
                    custom_system_prompt: None,
                    append_system_prompt: shared_clone.skill_prompt.clone(),
                    session_id: Some(shared_clone.session_id.clone()),
                    fallback_models: shared_clone.baoclaw_config.fallback_models.clone(),
                    max_retries_per_model: shared_clone.baoclaw_config.max_retries_per_model,
                })
            },
        ).await;

        let (client_id, broadcast_rx) = session.add_client().await;
        let msg_count = session.engine_read().await.get_messages().len();

        // Send init response with shared: true
        let _ = conn.send_response(init_id, serde_json::json!({
            "capabilities": { "tools": true, "streaming": true, "permissions": true },
            "session_id": &shared.session_id,
            "shared": true,
            "reconnected": msg_count > 0,
            "resumed": false,
            "message_count": msg_count,
        })).await;

        // Enter shared-mode RPC loop
        handle_shared_client(conn, shared, session.clone(), client_id, broadcast_rx, work_cwd, session_id_clone.clone()).await;

        // Client disconnect handling (Task 6.1)
        let is_last = session.remove_client(client_id).await;
        if is_last {
            shared_clone.session_registry.remove(&session_id_clone).await;
            eprintln!("Shared session '{}' removed (last client disconnected)", session_id_clone);
        }

        eprintln!("Shared client {} session ended", client_id);
        return;
    }

    // ── Independent mode (existing behavior) ──
    let mut engine = QueryEngine::new(QueryEngineConfig {
        cwd: work_cwd.clone(),
        tools: shared.engine_tools.clone(),
        api_client: Arc::clone(&shared.api_client),
        model: model.clone(),
        thinking_config: shared.cli_thinking_config.clone(),
        max_turns: None,
        max_budget_usd: None,
        verbose: false,
        custom_system_prompt: None,
        append_system_prompt: shared.skill_prompt.clone(),
        session_id: Some(shared.session_id.clone()),
        fallback_models: shared.baoclaw_config.fallback_models.clone(),
        max_retries_per_model: shared.baoclaw_config.max_retries_per_model,
    });

    // Handle session resume: explicit ID > CLI arg > auto-detect from cwd
    let resume_id = init_resume_session_id
        .or(shared.cli_resume_session_id.clone())
        .or_else(|| {
            // Auto-find the latest session for this project directory
            let cwd_str = work_cwd.to_string_lossy().to_string();
            engine::transcript::find_latest_session_for_cwd(&cwd_str)
        });
    let mut resumed = false;
    if let Some(ref resume_id) = resume_id {
        match TranscriptWriter::load(resume_id) {
            Ok(entries) => {
                let messages = rebuild_messages_from_transcript(&entries);
                if !messages.is_empty() {
                    engine.set_messages(messages);
                    resumed = true;
                    eprintln!("Resumed session {} ({} messages)", resume_id, engine.get_messages().len());
                }
            }
            Err(e) => {
                eprintln!("Failed to load transcript for session {}: {}", resume_id, e);
            }
        }
    }

    let msg_count = engine.get_messages().len();

    // Send init response
    let _ = conn.send_response(init_id, serde_json::json!({
        "capabilities": { "tools": true, "streaming": true, "permissions": true },
        "session_id": &shared.session_id,
        "reconnected": msg_count > 0,
        "resumed": resumed,
        "message_count": msg_count,
    })).await;

    // ── Client RPC loop ──
    while !shared.should_exit.load(Ordering::Relaxed) {
        let msg = match conn.recv_message().await {
            Ok(msg) => msg,
            Err(IpcError::ConnectionClosed) => {
                eprintln!("Client disconnected");
                break;
            }
            Err(e) => {
                eprintln!("IPC error: {}", e);
                break;
            }
        };

        match msg {
            JsonRpcMessage::Request(req) => {
                let id = req.id.clone();
                match parse_client_method(&req) {
                    Ok(method) => {
                        match method {
                            ClientMethod::SubmitMessage { prompt, attachments, .. } => {
                                let prompt_str = match prompt.as_str() {
                                    Some(s) => s.to_string(),
                                    None => serde_json::to_string(&prompt).unwrap_or_default(),
                                };
                                let mut rx = engine.submit_message_with_attachments(prompt_str, attachments).await;
                                let mut disconnected = false;
                                while let Some(event) = rx.recv().await {
                                    if send_engine_event(&mut conn, &event).await.is_err() {
                                        disconnected = true;
                                        break;
                                    }
                                    if matches!(event, EngineEvent::Result(_) | EngineEvent::Error(_)) {
                                        break;
                                    }
                                }
                                if disconnected {
                                    break;
                                }
                                engine.sync_messages().await;
                                let _ = conn.send_response(id, serde_json::json!({"status": "complete"})).await;
                            }
                            ClientMethod::Abort => {
                                engine.abort();
                                let _ = conn.send_response(id, serde_json::json!("ok")).await;
                            }
                            ClientMethod::Shutdown => {
                                let _ = conn.send_response(id, serde_json::json!("ok")).await;
                                shared.should_exit.store(true, Ordering::Relaxed);
                                break;
                            }
                            ClientMethod::UpdateSettings { settings } => {
                                if let Some(thinking) = settings.get("thinking") {
                                    if let Some(mode) = thinking.get("mode").and_then(|v| v.as_str()) {
                                        match mode {
                                            "enabled" => {
                                                let budget = thinking.get("budget_tokens")
                                                    .and_then(|v| v.as_u64())
                                                    .unwrap_or(10240) as u32;
                                                engine.update_thinking_config(ThinkingConfig::Enabled { budget_tokens: budget });
                                            }
                                            "adaptive" => {
                                                engine.update_thinking_config(ThinkingConfig::Adaptive);
                                            }
                                            _ => {
                                                engine.update_thinking_config(ThinkingConfig::Disabled);
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
                                let delivered = shared.permission_gate.respond(&tool_use_id, perm_decision);
                                let _ = conn.send_response(id, serde_json::json!({"delivered": delivered})).await;
                            }
                            ClientMethod::Initialize { .. } => {
                                let _ = conn.send_error(Some(id), -32600, "Already initialized".into()).await;
                            }
                            ClientMethod::ListTools => {
                                let tl: Vec<serde_json::Value> = shared.engine_tools.iter().map(|t| {
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
                                match engine.compact().await {
                                    Ok(result) => {
                                        let _ = conn.send_response(id, serde_json::json!({
                                            "tokens_saved": result.tokens_saved,
                                            "summary_tokens": result.summary_tokens,
                                            "tokens_before": result.tokens_before,
                                            "tokens_after": result.tokens_after,
                                        })).await;
                                    }
                                    Err(e) => {
                                        let _ = conn.send_error(Some(id), -32000, e.message).await;
                                    }
                                }
                            }
                            ClientMethod::SwitchModel { model: new_model } => {
                                engine.update_model(new_model.clone());
                                shared.state_manager.update(|s| { s.model = new_model.clone(); });
                                let _ = conn.send_response(id, serde_json::json!({"model": new_model})).await;
                            }
                            ClientMethod::SwitchCwd { cwd: new_cwd } => {
                                let abs_cwd = if new_cwd.is_absolute() {
                                    new_cwd
                                } else {
                                    work_cwd.join(&new_cwd)
                                };
                                if !abs_cwd.is_dir() {
                                    let _ = conn.send_error(Some(id), -32000,
                                        format!("Directory does not exist: {}", abs_cwd.display())).await;
                                } else {
                                    let baoclaw_dir = abs_cwd.join(".baoclaw");
                                    let created = if !baoclaw_dir.exists() {
                                        let _ = std::fs::create_dir_all(&baoclaw_dir);
                                        let _ = std::fs::write(baoclaw_dir.join("BAOCLAW.md"), "# Project Instructions\n\n");
                                        let _ = std::fs::write(baoclaw_dir.join("mcp.json"), "{\"mcpServers\":{}}\n");
                                        let _ = std::fs::create_dir_all(baoclaw_dir.join("skills"));
                                        true
                                    } else {
                                        false
                                    };
                                    engine.update_cwd(abs_cwd.clone());
                                    // Try to resume latest session for the new project
                                    let new_cwd_str = abs_cwd.to_string_lossy().to_string();
                                    if let Some(prev_session) = engine::transcript::find_latest_session_for_cwd(&new_cwd_str) {
                                        match TranscriptWriter::load(&prev_session) {
                                            Ok(entries) => {
                                                let messages = rebuild_messages_from_transcript(&entries);
                                                if !messages.is_empty() {
                                                    engine.set_messages(messages);
                                                    eprintln!("Resumed project session {} ({} messages)", prev_session, engine.get_messages().len());
                                                } else {
                                                    engine.set_messages(vec![]);
                                                }
                                            }
                                            Err(_) => { engine.set_messages(vec![]); }
                                        }
                                    } else {
                                        engine.set_messages(vec![]); // no previous session
                                    }
                                    shared.memory_store.switch_project(&abs_cwd).await;
                                    work_cwd = abs_cwd.clone();
                                    let msg_count = engine.get_messages().len();
                                    let _ = conn.send_response(id, serde_json::json!({
                                        "cwd": abs_cwd.display().to_string(),
                                        "scaffold_created": created,
                                        "message_count": msg_count
                                    })).await;
                                }
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
                                let add_result = tokio::process::Command::new("git")
                                    .args(["add", "-A"])
                                    .current_dir(&work_cwd)
                                    .output()
                                    .await;
                                match add_result {
                                    Ok(o) if o.status.success() => {
                                        let commit_result = tokio::process::Command::new("git")
                                            .args(["commit", "-m", &message])
                                            .current_dir(&work_cwd)
                                            .output()
                                            .await;
                                        match commit_result {
                                            Ok(co) if co.status.success() => {
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
                                let _ = conn.send_response(id, serde_json::json!({"resources": [], "count": 0})).await;
                            }
                            ClientMethod::ReadMcpResource { server_name, uri } => {
                                let _ = conn.send_error(Some(id), -32000,
                                    format!("MCP resource read not yet wired: {}:{}", server_name, uri)).await;
                            }
                            ClientMethod::TaskCreate { description, prompt } => {
                                let task_id = shared.task_manager.create_task(
                                    description,
                                    prompt,
                                    std::path::PathBuf::from(&work_cwd),
                                    shared.state_manager.get().model,
                                ).await;
                                let _ = conn.send_response(id, serde_json::json!({"task_id": task_id})).await;
                            }
                            ClientMethod::TaskList => {
                                let tasks = shared.task_manager.list_tasks().await;
                                let _ = conn.send_response(id, serde_json::json!({"tasks": tasks, "count": tasks.len()})).await;
                            }
                            ClientMethod::TaskStatus { task_id } => {
                                match shared.task_manager.get_task_status(&task_id).await {
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
                                let stopped = shared.task_manager.stop_task(&task_id).await;
                                let _ = conn.send_response(id, serde_json::json!({"stopped": stopped})).await;
                            }
                            ClientMethod::MemoryList => {
                                let entries = shared.memory_store.list().await;
                                let _ = conn.send_response(id, serde_json::json!({"memories": entries, "count": entries.len()})).await;
                            }
                            ClientMethod::MemoryAdd { content, category } => {
                                let cat = engine::memory::parse_category(&category);
                                let entry = shared.memory_store.add(content, cat, "user".to_string()).await;
                                let _ = conn.send_response(id, serde_json::json!({"memory": entry})).await;
                            }
                            ClientMethod::MemoryDelete { id: mem_id } => {
                                let deleted = shared.memory_store.delete(&mem_id).await;
                                let _ = conn.send_response(id, serde_json::json!({"deleted": deleted})).await;
                            }
                            ClientMethod::MemoryClear => {
                                let count = shared.memory_store.clear().await;
                                let _ = conn.send_response(id, serde_json::json!({"cleared": count})).await;
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

    eprintln!("Client session ended");
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
        let log_path = socket_path.with_extension("log");
        let log_file = std::fs::OpenOptions::new()
            .create(true).append(true).open(&log_path).ok();

        #[cfg(unix)]
        {
            use std::os::unix::io::AsRawFd;
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
    let api_client: Arc<UnifiedClient> = {
        match baoclaw_config.api_type.as_str() {
            "openai" => {
                let base_url = std::env::var("OPENAI_BASE_URL").ok()
                    .or(baoclaw_config.openai_base_url.clone())
                    .or_else(|| std::env::var("ANTHROPIC_BASE_URL").ok());
                let api_key_openai = std::env::var("OPENAI_API_KEY")
                    .unwrap_or_else(|_| api_key.clone());
                eprintln!("Using OpenAI-compatible API (base_url: {})", base_url.as_deref().unwrap_or("default"));
                let config = ApiClientConfig {
                    api_key: api_key_openai,
                    base_url,
                    max_retries: None,
                };
                Arc::new(UnifiedClient::new_openai(config))
            }
            _ => {
                eprintln!("Using Anthropic-compatible API");
                let config = ApiClientConfig {
                    api_key,
                    base_url: std::env::var("ANTHROPIC_BASE_URL").ok(),
                    max_retries: None,
                };
                Arc::new(UnifiedClient::new_anthropic(config))
            }
        }
    };

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
        Arc::new(MemoryTool::new()),
        Arc::new(ProjectNoteTool::new()),
        Arc::new(AgentTool::new(Arc::clone(&api_client), read_only_tools)),
    ];

    // ToolSearchTool needs the full tool list, so register it last
    let engine_tools: Vec<Arc<dyn tools::Tool>> = {
        let mut all = engine_tools;

        // MCP integration: discover and connect to MCP servers (with timeout)
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
                let connect_result = tokio::time::timeout(
                    std::time::Duration::from_secs(30),
                    client.connect_stdio(),
                ).await;
                match connect_result {
                    Ok(Ok(())) => {
                        let client = Arc::new(client);
                        if let Ok(tools) = client.list_tools().await {
                            eprintln!("MCP server '{}': {} tools discovered", server_info.name, tools.len());
                            for tool_def in &tools {
                                eprintln!("  MCP tool: {}", tool_def.name);
                            }
                            for tool_def in tools {
                                let wrapper = McpToolWrapper::new(
                                    Arc::clone(&client),
                                    tool_def,
                                    server_info.name.clone(),
                                );
                                all.push(Arc::new(wrapper));
                            }
                        } else {
                            eprintln!("MCP server '{}': list_tools failed", server_info.name);
                        }
                        eprintln!("MCP server '{}' connected", server_info.name);
                    }
                    Ok(Err(e)) => {
                        eprintln!("Warning: MCP server '{}' failed to connect: {}", server_info.name, e);
                    }
                    Err(_) => {
                        eprintln!("Warning: MCP server '{}' connection timed out (30s)", server_info.name);
                    }
                }
            }
        }

        all.push(Arc::new(ToolSearchTool::new(all.clone())));
        eprintln!("Total tools registered: {} (including MCP)", all.len());
        all
    };

    // Load skill content for system prompt injection
    let skill_prompt = discovery::skills::load_skills_for_prompt(std::path::Path::new(&cwd_str)).await;
    if let Some(ref sp) = skill_prompt {
        eprintln!("Loaded skills into system prompt ({} chars)", sp.len());
    }

    // Load long-term memory
    let memory_store = Arc::new(engine::memory::MemoryStore::load());
    let memory_prompt = memory_store.build_prompt_fragment().await;
    if let Some(ref mp) = memory_prompt {
        eprintln!("Loaded long-term memory into system prompt ({} chars)", mp.len());
    }

    // Combine skill + memory into append_system_prompt
    let combined_append_prompt = {
        let mut parts = Vec::new();
        if let Some(sp) = skill_prompt { parts.push(sp); }
        if let Some(mp) = memory_prompt { parts.push(mp); }
        if parts.is_empty() { None } else { Some(parts.join("\n\n")) }
    };

    // Session ID: cwd-hash prefix + short UUID for uniqueness
    // This ties sessions to projects so they can be auto-resumed per-project.
    let cwd_hash = &format!("{:x}", md5_simple(&cwd_str))[..8];
    let session_id = format!("{}-{}", cwd_hash, &uuid::Uuid::new_v4().to_string()[..8]);

    // Write metadata file for discovery by CLI
    write_meta(&socket_path, &cwd_str, &session_id);

    let state_manager = Arc::new(StateManager::new(CoreState {
        session_id: session_id.clone(),
        model: baoclaw_config.model.clone(),
        verbose: false,
        tasks: std::collections::HashMap::new(),
        usage: EMPTY_USAGE,
        total_cost_usd: 0.0,
    }));

    // If daemon mode, ignore SIGHUP so we survive terminal close
    if is_daemon {
        #[cfg(unix)]
        unsafe {
            libc::signal(libc::SIGHUP, libc::SIG_IGN);
        }
    }

    let should_exit = Arc::new(AtomicBool::new(false));

    // Create PermissionGate for the permission interactive flow
    let permission_gate = PermissionGate::new();

    // Create TaskManager for background task execution
    let task_manager = Arc::new(TaskManager::new(
        Arc::clone(&api_client),
        engine_tools.clone(),
    ));

    let shared = SharedState {
        engine_tools,
        api_client,
        permission_gate,
        task_manager,
        state_manager,
        baoclaw_config,
        cli_thinking_config,
        cli_resume_session_id,
        session_id,
        should_exit: Arc::clone(&should_exit),
        session_registry: Arc::new(SessionRegistry::new()),
        skill_prompt: combined_append_prompt,
        memory_store,
    };

    // ══════════════════════════════════════════════════════════
    // Main accept loop — spawns a task per client connection
    // Multiple clients can be connected simultaneously, each
    // with its own independent QueryEngine / conversation history.
    // Only `shutdown` RPC terminates the daemon.
    // ══════════════════════════════════════════════════════════
    let should_exit_clone = Arc::clone(&should_exit);
    loop {
        if should_exit.load(Ordering::Relaxed) {
            break;
        }
        eprintln!("Waiting for client connection...");

        // Use select to race accept against a periodic should_exit check
        // so shutdown actually terminates the daemon promptly
        let accept_result = tokio::select! {
            result = server.accept() => Some(result),
            _ = async {
                loop {
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                    if should_exit_clone.load(Ordering::Relaxed) {
                        break;
                    }
                }
            } => None,
        };

        match accept_result {
            None => break, // should_exit was set
            Some(Ok(conn)) => {
                eprintln!("Client connected");
                let client_shared = shared.clone();
                tokio::spawn(async move {
                    handle_client(conn, client_shared).await;
                });
            }
            Some(Err(e)) => {
                eprintln!("Accept error: {}", e);
                continue;
            }
        }
    }

    // Cleanup
    cleanup_meta(&socket_path);
    drop(server);
    eprintln!("baoclaw-core shutdown complete");
    Ok(())
}
