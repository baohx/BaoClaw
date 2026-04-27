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
    evolution_engine: Arc<engine::evolution::EvolutionEngine>,
    cron_manager: Arc<engine::cron::CronManager>,
    project_registry: Arc<engine::projects::ProjectRegistry>,
}

/// Socket directory for all BaoClaw daemon instances
fn socket_dir() -> PathBuf {
    let dir = std::env::temp_dir().join("baoclaw-sockets");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// Generate a socket path with session info embedded in the filename
fn make_socket_path(_cwd: &str) -> PathBuf {
    let pid = std::process::id();
    socket_dir().join(format!("baoclaw-{}.sock", pid))
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
    mut work_cwd: PathBuf,
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
                                    if !baoclaw_dir.exists() {
                                        let _ = std::fs::create_dir_all(&baoclaw_dir);
                                        let _ = std::fs::write(baoclaw_dir.join("BAOCLAW.md"), "# Project Instructions\n\n");
                                        let _ = std::fs::write(baoclaw_dir.join("mcp.json"), "{\"mcpServers\":{}}\n");
                                        let _ = std::fs::create_dir_all(baoclaw_dir.join("skills"));
                                    }
                                    let mut engine = session.engine_write().await;
                                    engine.update_cwd(abs_cwd.clone());
                                    let new_session_key = format!("{:x}", md5_simple(&abs_cwd.to_string_lossy()))[..8].to_string();
                                    engine.update_session_id(new_session_key);
                                    let mut conn_guard = conn.lock().await;
                                    let _ = conn_guard.send_response(id, serde_json::json!({
                                        "cwd": abs_cwd.display().to_string()
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
                            ClientMethod::CronAdd { name, prompt, schedule, cwd } => {
                                let mut conn_guard = conn.lock().await;
                                match shared.cron_manager.add_job(name, prompt, schedule, cwd).await {
                                    Ok(job) => {
                                        let _ = conn_guard.send_response(id, serde_json::json!({"job": job})).await;
                                    }
                                    Err(e) => {
                                        let _ = conn_guard.send_error(Some(id), -32000, e).await;
                                    }
                                }
                            }
                            ClientMethod::CronRemove { id: job_id } => {
                                let removed = shared.cron_manager.remove_job(&job_id).await;
                                let mut conn_guard = conn.lock().await;
                                let _ = conn_guard.send_response(id, serde_json::json!({"removed": removed})).await;
                            }
                            ClientMethod::CronToggle { id: job_id } => {
                                let mut conn_guard = conn.lock().await;
                                match shared.cron_manager.toggle_job(&job_id).await {
                                    Some(enabled) => {
                                        let _ = conn_guard.send_response(id, serde_json::json!({"enabled": enabled})).await;
                                    }
                                    None => {
                                        let _ = conn_guard.send_error(Some(id), -32000, "Job not found".to_string()).await;
                                    }
                                }
                            }
                            ClientMethod::CronList => {
                                let jobs = shared.cron_manager.list_jobs().await;
                                let mut conn_guard = conn.lock().await;
                                let _ = conn_guard.send_response(id, serde_json::json!({"jobs": jobs, "count": jobs.len()})).await;
                            }
                            ClientMethod::ProjectsList => {
                                let projects = shared.project_registry.list().await;
                                // Enrich each project with its session_id (derived from cwd hash)
                                let enriched: Vec<serde_json::Value> = projects.iter().map(|p| {
                                    let session_key = format!("{:x}", md5_simple(&p.cwd))[..8].to_string();
                                    let mut v = serde_json::to_value(p).unwrap_or_default();
                                    v["session_id"] = serde_json::json!(session_key);
                                    v
                                }).collect();
                                let count = enriched.len();
                                let mut conn_guard = conn.lock().await;
                                let _ = conn_guard.send_response(id, serde_json::json!({"projects": enriched, "count": count})).await;
                            }
                            ClientMethod::ProjectsSwitch { id_prefix } => {
                                let mut conn_guard = conn.lock().await;
                                match shared.project_registry.find_by_prefix(&id_prefix).await {
                                    Ok(project) => {
                                        let abs_cwd = std::path::PathBuf::from(&project.cwd);
                                        if !abs_cwd.is_dir() {
                                            let _ = conn_guard.send_error(Some(id), -32000,
                                                format!("Directory does not exist: {}", project.cwd)).await;
                                        } else {
                                            // Switch session: update engine cwd and reload
                                            let mut engine = session.engine_write().await;
                                            engine.update_cwd(abs_cwd.clone());
                                            // Update session_id so transcript writes go to the correct file
                                            let new_session_key = format!("{:x}", md5_simple(&abs_cwd.to_string_lossy()))[..8].to_string();
                                            engine.update_session_id(new_session_key.clone());
                                            let new_cwd_str = abs_cwd.to_string_lossy().to_string();
                                            if let Some(prev_session) = engine::transcript::find_latest_session_for_cwd(&new_cwd_str) {
                                                if let Ok(entries) = engine::transcript::TranscriptWriter::load(&prev_session) {
                                                    let messages = engine::transcript::rebuild_messages_from_transcript(&entries);
                                                    engine.set_messages(messages);
                                                }
                                            } else {
                                                engine.set_messages(vec![]);
                                            }
                                            drop(engine);
                                            shared.memory_store.switch_project(&abs_cwd).await;
                                            shared.project_registry.touch(&project.cwd).await;
                                            work_cwd = abs_cwd.clone();
                                            let msg_count = session.engine_read().await.get_messages().len();
                                            let _ = conn_guard.send_response(id, serde_json::json!({
                                                "project": project,
                                                "message_count": msg_count,
                                                "session_id": new_session_key,
                                            })).await;
                                        }
                                    }
                                    Err(e) => {
                                        let _ = conn_guard.send_error(Some(id), -32000, e).await;
                                    }
                                }
                            }
                            ClientMethod::ProjectsNew { cwd, description } => {
                                let expanded = if cwd.starts_with('~') {
                                    let home = std::env::var("HOME").unwrap_or_default();
                                    cwd.replacen('~', &home, 1)
                                } else if std::path::Path::new(&cwd).is_relative() {
                                    work_cwd.join(&cwd).to_string_lossy().to_string()
                                } else {
                                    cwd.clone()
                                };
                                let abs_path = std::path::PathBuf::from(&expanded);
                                if !abs_path.is_dir() {
                                    let mut conn_guard = conn.lock().await;
                                    let _ = conn_guard.send_error(Some(id), -32000,
                                        format!("Directory does not exist: {}", expanded)).await;
                                } else {
                                    let desc = description.unwrap_or_else(|| {
                                        abs_path.file_name()
                                            .map(|n| n.to_string_lossy().to_string())
                                            .unwrap_or_else(|| expanded.clone())
                                    });
                                    let mut conn_guard = conn.lock().await;
                                    match shared.project_registry.register(expanded.clone(), desc).await {
                                        Ok(project) => {
                                            // Auto-scaffold
                                            let baoclaw_dir = abs_path.join(".baoclaw");
                                            if !baoclaw_dir.exists() {
                                                let _ = std::fs::create_dir_all(&baoclaw_dir);
                                                let _ = std::fs::write(baoclaw_dir.join("BAOCLAW.md"), "# Project Instructions\n\n");
                                                let _ = std::fs::write(baoclaw_dir.join("mcp.json"), "{\"mcpServers\":{}}\n");
                                                let _ = std::fs::create_dir_all(baoclaw_dir.join("skills"));
                                            }
                                            // Switch to the new project
                                            let mut engine = session.engine_write().await;
                                            engine.update_cwd(abs_path.clone());
                                            let new_session_key = format!("{:x}", md5_simple(&abs_path.to_string_lossy()))[..8].to_string();
                                            engine.update_session_id(new_session_key);
                                            engine.set_messages(vec![]);
                                            drop(engine);
                                            shared.memory_store.switch_project(&abs_path).await;
                                            work_cwd = abs_path;
                                            let _ = conn_guard.send_response(id, serde_json::json!({
                                                "project": project,
                                                "switched": true,
                                            })).await;
                                        }
                                        Err(e) => {
                                            let _ = conn_guard.send_error(Some(id), -32000, e).await;
                                        }
                                    }
                                }
                            }
                            ClientMethod::ProjectsUpdateDesc { id_prefix, description } => {
                                let mut conn_guard = conn.lock().await;
                                match shared.project_registry.update_description(&id_prefix, description).await {
                                    Ok(()) => {
                                        let _ = conn_guard.send_response(id, serde_json::json!({"updated": true})).await;
                                    }
                                    Err(e) => {
                                        let _ = conn_guard.send_error(Some(id), -32000, e).await;
                                    }
                                }
                            }
                            ClientMethod::TalkTail { count } => {
                                let engine = session.engine_read().await;
                                let messages = engine.get_messages();
                                let start = if messages.len() > count { messages.len() - count } else { 0 };
                                let tail: Vec<serde_json::Value> = messages[start..].iter().map(|m| {
                                    match &m.content {
                                        crate::models::message::MessageContent::User { message, .. } => {
                                            let text = match &message.content {
                                                serde_json::Value::String(s) => s.clone(),
                                                serde_json::Value::Array(arr) => {
                                                    arr.iter().filter_map(|b| {
                                                        if b.get("type").and_then(|t| t.as_str()) == Some("text") {
                                                            b.get("text").and_then(|t| t.as_str()).map(|s| s.to_string())
                                                        } else { None }
                                                    }).collect::<Vec<_>>().join(" ")
                                                }
                                                _ => serde_json::to_string(&message.content).unwrap_or_default(),
                                            };
                                            serde_json::json!({"role": "user", "text": text, "timestamp": m.timestamp})
                                        }
                                        crate::models::message::MessageContent::Assistant { message, .. } => {
                                            let text: String = message.content.iter().filter_map(|b| {
                                                match b {
                                                    crate::models::message::ContentBlock::Text { text } => Some(text.clone()),
                                                    _ => None,
                                                }
                                            }).collect::<Vec<_>>().join("");
                                            let tools: Vec<serde_json::Value> = message.content.iter().filter_map(|b| {
                                                match b {
                                                    crate::models::message::ContentBlock::ToolUse { name, input, .. } => {
                                                        // Include tool name + key input params for richer history display
                                                        let mut info = serde_json::json!({"name": name});
                                                        if let Some(cmd) = input.get("command").and_then(|v| v.as_str()) {
                                                            info["detail"] = serde_json::json!(cmd.chars().take(120).collect::<String>());
                                                        } else if let Some(fp) = input.get("file_path").and_then(|v| v.as_str()) {
                                                            info["detail"] = serde_json::json!(fp);
                                                        } else if let Some(p) = input.get("pattern").and_then(|v| v.as_str()) {
                                                            info["detail"] = serde_json::json!(p);
                                                        } else if let Some(q) = input.get("query").and_then(|v| v.as_str()) {
                                                            info["detail"] = serde_json::json!(q);
                                                        } else if let Some(u) = input.get("url").and_then(|v| v.as_str()) {
                                                            info["detail"] = serde_json::json!(u.chars().take(80).collect::<String>());
                                                        }
                                                        Some(info)
                                                    }
                                                    _ => None,
                                                }
                                            }).collect();
                                            let mut entry = serde_json::json!({"role": "assistant", "text": text, "timestamp": m.timestamp});
                                            if !tools.is_empty() {
                                                entry["tools"] = serde_json::json!(tools);
                                            }
                                            entry
                                        }
                                        _ => serde_json::json!({"role": "system", "timestamp": m.timestamp}),
                                    }
                                }).collect();
                                let total = messages.len();
                                let mut conn_guard = conn.lock().await;
                                let _ = conn_guard.send_response(id, serde_json::json!({
                                    "messages": tail,
                                    "count": tail.len(),
                                    "total": total,
                                })).await;
                            }
                            ClientMethod::SearchHistory { query, max_results } => {
                                // Search through current session messages for matching text
                                let engine = session.engine_read().await;
                                let messages = engine.get_messages();
                                let query_lower = query.to_lowercase();
                                let mut results: Vec<serde_json::Value> = Vec::new();

                                for m in messages.iter().rev() {
                                    if results.len() >= max_results { break; }
                                    let (role, text) = match &m.content {
                                        crate::models::message::MessageContent::User { message, .. } => {
                                            let t = match &message.content {
                                                serde_json::Value::String(s) => s.clone(),
                                                serde_json::Value::Array(arr) => arr.iter().filter_map(|b| b.get("text").and_then(|t| t.as_str()).map(String::from)).collect::<Vec<_>>().join(" "),
                                                _ => String::new(),
                                            };
                                            ("user", t)
                                        }
                                        crate::models::message::MessageContent::Assistant { message, .. } => {
                                            let t: String = message.content.iter().filter_map(|b| match b {
                                                crate::models::message::ContentBlock::Text { text } => Some(text.clone()),
                                                _ => None,
                                            }).collect::<Vec<_>>().join(" ");
                                            ("assistant", t)
                                        }
                                        _ => continue,
                                    };
                                    if text.to_lowercase().contains(&query_lower) {
                                        // Extract a snippet around the match
                                        let lower = text.to_lowercase();
                                        let idx = lower.find(&query_lower).unwrap_or(0);
                                        let start = idx.saturating_sub(50);
                                        let end = (idx + query.len() + 100).min(text.len());
                                        let snippet = &text[start..end];
                                        results.push(serde_json::json!({
                                            "role": role,
                                            "text": text.chars().take(200).collect::<String>(),
                                            "snippet": snippet,
                                            "timestamp": m.timestamp,
                                        }));
                                    }
                                }

                                let mut conn_guard = conn.lock().await;
                                let _ = conn_guard.send_response(id, serde_json::json!({
                                    "results": results,
                                    "count": results.len(),
                                    "query": query,
                                })).await;
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

    let (init_id, init_cwd, init_model, _init_resume_session_id, init_shared_session_id) = match init_msg {
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
    let work_cwd = init_cwd;

    // ── Shared mode: session key is derived from cwd, not client-provided ID ──
    // This allows one daemon to manage multiple project sessions.
    if let Some(ref shared_session_id) = init_shared_session_id {
        // Session key = cwd_hash + client_type, so different clients (web/telegram/cli)
        // on the same cwd get independent sessions and don't block each other.
        let cwd_hash = format!("{:x}", md5_simple(&work_cwd.to_string_lossy()))[..8].to_string();
        let session_id_clone = format!("{}-{}", cwd_hash, shared_session_id);
        eprintln!("Client connecting to session '{}' (cwd: {})", session_id_clone, work_cwd.display());
        let shared_clone = shared.clone();
        let model_clone = model.clone();
        let work_cwd_clone = work_cwd.clone();

        let (session, is_new) = shared.session_registry.get_or_create(
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
                    session_id: Some(session_id_clone.clone()),
                    fallback_models: shared_clone.baoclaw_config.fallback_models.clone(),
                    max_retries_per_model: shared_clone.baoclaw_config.max_retries_per_model,
                })
            },
        ).await;

        // Auto-register this project in the registry
        shared.project_registry.ensure_registered(
            &work_cwd.to_string_lossy(), None
        ).await;

        // Resume session history if new or empty
        let current_msg_count = session.engine_read().await.get_messages().len();
        if is_new || current_msg_count == 0 {
            let cwd_str_for_resume = work_cwd.to_string_lossy().to_string();
            if let Some(rid) = engine::transcript::find_latest_session_for_cwd(&cwd_str_for_resume) {
                match engine::transcript::TranscriptWriter::load(&rid) {
                    Ok(entries) => {
                        let messages = engine::transcript::rebuild_messages_from_transcript(&entries);
                        if !messages.is_empty() {
                            let mut engine = session.engine_write().await;
                            engine.set_messages(messages);
                            eprintln!("Resumed session {} ({} messages)", rid, engine.get_messages().len());
                        }
                    }
                    Err(e) => eprintln!("Failed to resume session {}: {}", rid, e),
                }
            }
        }

        let (client_id, broadcast_rx) = session.add_client().await;
        let msg_count = session.engine_read().await.get_messages().len();

        // Send init response with shared: true
        let _ = conn.send_response(init_id, serde_json::json!({
            "capabilities": { "tools": true, "streaming": true, "permissions": true },
            "session_id": &session_id_clone,
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

    // All clients use shared mode.
    eprintln!("Client disconnected: no shared_session_id provided");
    let _ = conn.send_error(Some(init_id), -32600,
        "shared_session_id is required".into()).await;
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
    // api_type in config.json determines which env vars to use:
    //   "openai"    → OPENAI_API_KEY, OPENAI_BASE_URL
    //   "anthropic" → ANTHROPIC_API_KEY, ANTHROPIC_BASE_URL
    let api_client: Arc<UnifiedClient> = {
        match baoclaw_config.api_type.as_str() {
            "openai" => {
                let api_key = std::env::var("OPENAI_API_KEY").unwrap_or_default();
                let base_url = std::env::var("OPENAI_BASE_URL").ok();
                eprintln!("Using OpenAI-compatible API (model: {}, base_url: {})",
                    baoclaw_config.model,
                    base_url.as_deref().unwrap_or("https://api.openai.com"));
                let config = ApiClientConfig {
                    api_key,
                    base_url,
                    max_retries: None,
                };
                Arc::new(UnifiedClient::new_openai(config))
            }
            _ => {
                let api_key = std::env::var("ANTHROPIC_API_KEY").unwrap_or_default();
                let base_url = std::env::var("ANTHROPIC_BASE_URL").ok();
                eprintln!("Using Anthropic API (model: {}, base_url: {})",
                    baoclaw_config.model,
                    base_url.as_deref().unwrap_or("https://api.anthropic.com"));
                let config = ApiClientConfig {
                    api_key,
                    base_url,
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

    // Create evolution engine for self-improvement
    let evolution_engine = Arc::new(engine::evolution::EvolutionEngine::new(std::path::Path::new(&cwd_str)));

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
        Arc::new(tools::builtins::EvolveTool::new(Arc::clone(&evolution_engine))),
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

    // Reuse existing project session or create new one.
    // One project directory = one session file.
    let cwd_hash = &format!("{:x}", md5_simple(&cwd_str))[..8];
    let session_id = engine::transcript::find_latest_session_for_cwd(&cwd_str)
        .unwrap_or_else(|| format!("{}-{}", cwd_hash, &uuid::Uuid::new_v4().to_string()[..8]));
    eprintln!("Session ID: {} (cwd: {})", session_id, cwd_str);

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
        evolution_engine,
        cron_manager: Arc::new(engine::cron::CronManager::new()),
        project_registry: Arc::new(engine::projects::ProjectRegistry::new()),
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
