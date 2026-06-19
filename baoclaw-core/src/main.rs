use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::path::PathBuf;
use tokio::sync::Mutex as TokioMutex;

mod api;
mod bridge;
mod config;
mod discovery;
mod doc_upload;
mod engine;
mod ipc;
mod mcp;
mod models;
mod permissions;
mod state;
mod telemetry;
mod tools;
mod updater;
mod utils;

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
use tools::builtins::{AgentTool, BashTool, FileEditTool, FileReadTool, FileWriteTool, ImageEditTool, ImageGenTool, MemoryTool, NotebookEditTool, ProjectNoteTool, TodoWriteTool, ToolSearchTool, WebFetchTool, WebSearchTool};
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
    memory_archive: Arc<engine::memory::MemoryArchive>,
    memory_cleanup: Arc<engine::memory::MemoryCleanupScheduler>,
    evolution_engine: Arc<engine::evolution::EvolutionEngine>,
    cron_manager: Arc<engine::cron::CronManager>,
    project_registry: Arc<engine::projects::ProjectRegistry>,
    /// Shared file cache (LRU) for reducing redundant file reads.
    file_cache: Arc<tokio::sync::Mutex<engine::file_cache::FileCache>>,
    /// Tool result store for persisting large outputs to disk.
    tool_result_store: Option<Arc<engine::tool_result_store::ToolResultStore>>,
    /// Hook manager for event-driven automation.
    hook_manager: Arc<engine::hooks::HookManager>,
    /// Team executor for managing sub-agent teams.
    team_executor: Arc<engine::team::TeamManager>,
}

/// Socket directory for all BaoClaw daemon instances
fn socket_dir() -> PathBuf {
    let dir = std::env::temp_dir().join("baoclaw-sockets");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// Compute a stable hash of the working directory path.
/// Uses SipHash-1-3 (via DefaultHasher), 64-bit output truncated to 16 hex chars
/// for a short, collision-resistant ID.
fn cwd_hash(cwd: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    cwd.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn make_socket_path(cwd: &str) -> PathBuf {
    let hash = cwd_hash(cwd);
    socket_dir().join(format!("baoclaw-cwd-{}.sock", hash))
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

// ══════════════════════════════════════════════════════════
// Loop Header rendering for TUI
// ══════════════════════════════════════════════════════════

/// Render a Loop Header line when a new turn starts.
/// Outputs a formatted line like: `── Loop 1 ──`
/// If an agent_label is provided, includes it: `── Loop 1 (agent: sub) ──`
fn render_loop_header(turn_id: u32, agent_label: Option<&str>) {
    let label_part = match agent_label {
        Some(label) => format!(" ({})", label),
        None => String::new(),
    };
    eprintln!("── Loop {}{} ──", turn_id, label_part);
}

/// Update the Loop Header with statistics when a turn ends.
/// Outputs a formatted line like: `── Loop 1 ── tools: 3, 2.1s ──`
fn update_loop_header(turn_id: u32, tool_count: u32, duration_ms: u64) {
    let duration_secs = duration_ms as f64 / 1000.0;
    eprintln!("── Loop {} ── tools: {}, {:.1}s ──", turn_id, tool_count, duration_secs);
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

    // Spawn background task to forward cron results to this client.
    // Cron jobs run independently (not tied to any session), so their
    // results are delivered via a separate broadcast channel.
    let conn_for_cron = Arc::clone(&conn);
    let mut cron_rx = shared.cron_manager.subscribe();
    let cron_broadcast_handle = tokio::spawn(async move {
        loop {
            match cron_rx.recv().await {
                Ok(cron_result) => {
                    let mut conn_guard = conn_for_cron.lock().await;
                    let params = serde_json::json!({
                        "job_id": cron_result.job_id,
                        "job_name": cron_result.job_name,
                        "text": cron_result.text,
                        "timestamp": cron_result.timestamp,
                    });
                    if conn_guard.send_notification("cron_result", params).await.is_err() {
                        break; // Client disconnected
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    eprintln!("Cron result receiver lagged by {} events", n);
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
                                    // Render Loop Headers to TUI on turn events
                                    match &event {
                                        EngineEvent::TurnStart { turn_id, agent_label, .. } => {
                                            render_loop_header(*turn_id, agent_label.as_deref());
                                        }
                                        EngineEvent::TurnEnd { turn_id, tool_count, duration_ms, .. } => {
                                            update_loop_header(*turn_id, *tool_count, *duration_ms);
                                        }
                                        _ => {}
                                    }

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
                                        let mut engine = session.engine_write().await;
                                        engine.sync_messages().await;
                                        break;
                                    }
                                }

                                // Release submitter AFTER the loop ends to prevent
                                // the broadcast task from re-delivering the Result event.
                                session.release_submitter(client_id).await;

                                if disconnected {
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
                                eprintln!("Shutdown requested — setting should_exit flag");
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
                                let result = shared.memory_store.add(content, cat, "user".to_string()).await;
                                let mut conn_guard = conn.lock().await;
                                match result {
                                    Ok(entry) => {
                                        let _ = conn_guard.send_response(id, serde_json::json!({"memory": entry})).await;
                                    }
                                    Err(e) => {
                                        eprintln!("ERROR: memory add failed: {}", e);
                                        let _ = conn_guard.send_error(Some(id), -32000, format!("Memory write failed: {}", e)).await;
                                    }
                                }
                            }
                            ClientMethod::MemoryDelete { id: mem_id } => {
                                let result = shared.memory_store.delete(&mem_id).await;
                                let mut conn_guard = conn.lock().await;
                                match result {
                                    Ok(deleted) => {
                                        let _ = conn_guard.send_response(id, serde_json::json!({"deleted": deleted})).await;
                                    }
                                    Err(e) => {
                                        eprintln!("ERROR: memory delete failed: {}", e);
                                        let _ = conn_guard.send_error(Some(id), -32000, format!("Memory delete failed: {}", e)).await;
                                    }
                                }
                            }
                            ClientMethod::MemoryClear => {
                                let result = shared.memory_store.clear().await;
                                let mut conn_guard = conn.lock().await;
                                match result {
                                    Ok(count) => {
                                        let _ = conn_guard.send_response(id, serde_json::json!({"cleared": count})).await;
                                    }
                                    Err(e) => {
                                        eprintln!("ERROR: memory clear failed: {}", e);
                                        let _ = conn_guard.send_error(Some(id), -32000, format!("Memory clear failed: {}", e)).await;
                                    }
                                }
                            }
                            ClientMethod::MemoryStats => {
                                let stats = shared.memory_store.stats().await;
                                let mut conn_guard = conn.lock().await;
                                let _ = conn_guard.send_response(id, serde_json::json!(stats)).await;
                            }
                            ClientMethod::MemoryArchive { id: mem_id } => {
                                let archived = shared.memory_store.archive_by_id(&mem_id, &shared.memory_archive).await;
                                let mut conn_guard = conn.lock().await;
                                match archived {
                                    Some(entry) => {
                                        let _ = conn_guard.send_response(id, serde_json::json!({"archived": entry})).await;
                                    }
                                    None => {
                                        let _ = conn_guard.send_error(Some(id), -32000, format!("Memory not found: {}", mem_id)).await;
                                    }
                                }
                            }
                            ClientMethod::MemoryRestore { id: mem_id } => {
                                let restored = shared.memory_store.restore_from_archive(&mem_id, &shared.memory_archive).await;
                                let mut conn_guard = conn.lock().await;
                                match restored {
                                    Some(entry) => {
                                        let _ = conn_guard.send_response(id, serde_json::json!({"restored": entry})).await;
                                    }
                                    None => {
                                        let _ = conn_guard.send_error(Some(id), -32000, format!("Archived memory not found: {}", mem_id)).await;
                                    }
                                }
                            }
                            ClientMethod::MemoryArchiveList => {
                                let archived = shared.memory_archive.list_archived().await;
                                let mut conn_guard = conn.lock().await;
                                let _ = conn_guard.send_response(id, serde_json::json!({"archived": archived, "count": archived.len()})).await;
                            }
                            ClientMethod::MemoryCleanup => {
                                let result = shared.memory_cleanup.run_now().await;
                                let mut conn_guard = conn.lock().await;
                                let _ = conn_guard.send_response(id, serde_json::json!({
                                    "archived_count": result.archived_count,
                                    "deleted_count": result.deleted_count,
                                    "timestamp": result.timestamp,
                                    "duration_ms": result.duration_ms,
                                })).await;
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
                                // Collect tool results from user messages to attach to tool_use blocks
                                let tool_results: std::collections::HashMap<String, serde_json::Value> = messages[start..].iter()
                                    .filter_map(|m| match &m.content {
                                        crate::models::message::MessageContent::User { tool_use_result, .. } => {
                                            tool_use_result.as_ref().map(|r| (r.tool_use_id.clone(), r.output.clone()))
                                        }
                                        _ => None,
                                    }).collect();

                                let tail: Vec<serde_json::Value> = messages[start..].iter().enumerate().map(|(idx, m)| {
                                    let turn_num = start + idx + 1;
                                    match &m.content {
                                        crate::models::message::MessageContent::User { message, tool_use_result, .. } => {
                                            let text = match &message.content {
                                                serde_json::Value::String(s) => s.clone(),
                                                serde_json::Value::Array(arr) => {
                                                    arr.iter().filter_map(|b| {
                                                        if b.get("type").and_then(|t| t.as_str()) == Some("text") {
                                                            b.get("text").and_then(|t| t.as_str()).map(|s| s.to_string())
                                                        } else if b.get("type").and_then(|t| t.as_str()) == Some("image") {
                                                            Some("[image]".to_string())
                                                        } else if b.get("type").and_then(|t| t.as_str()) == Some("document") {
                                                            Some("[document]".to_string())
                                                        } else { None }
                                                    }).collect::<Vec<_>>().join(" ")
                                                }
                                                _ => serde_json::to_string(&message.content).unwrap_or_default(),
                                            };
                                            let is_tool_result = tool_use_result.is_some();
                                            let mut entry = serde_json::json!({
                                                "role": "user",
                                                "text": text,
                                                "timestamp": m.timestamp,
                                                "turn": turn_num,
                                            });
                                            if is_tool_result {
                                                entry["is_tool_result"] = serde_json::json!(true);
                                                if let Some(tr) = tool_use_result {
                                                    entry["tool_use_id"] = serde_json::json!(tr.tool_use_id);
                                                    entry["result_output"] = tr.output.clone();
                                                    entry["is_error"] = serde_json::json!(tr.is_error);
                                                }
                                            }
                                            entry
                                        }
                                        crate::models::message::MessageContent::Assistant { message, cost_usd, duration_ms } => {
                                            let text: String = message.content.iter().filter_map(|b| {
                                                match b {
                                                    crate::models::message::ContentBlock::Text { text } => Some(text.clone()),
                                                    _ => None,
                                                }
                                            }).collect::<Vec<_>>().join("");
                                            let tools: Vec<serde_json::Value> = message.content.iter().filter_map(|b| {
                                                match b {
                                                    crate::models::message::ContentBlock::ToolUse { id, name, input } => {
                                                        let mut info = serde_json::json!({"name": name, "id": id});
                                                        let mut details: Vec<String> = Vec::new();
                                                        if let Some(cmd) = input.get("command").and_then(|v| v.as_str()) {
                                                            details.push(format!("command: {}", cmd));
                                                        }
                                                        if let Some(fp) = input.get("file_path").and_then(|v| v.as_str()) {
                                                            details.push(format!("path: {}", fp));
                                                        }
                                                        if let Some(p) = input.get("pattern").and_then(|v| v.as_str()) {
                                                            details.push(format!("pattern: {}", p));
                                                        }
                                                        if let Some(q) = input.get("query").and_then(|v| v.as_str()) {
                                                            details.push(format!("query: {}", q));
                                                        }
                                                        if let Some(u) = input.get("url").and_then(|v| v.as_str()) {
                                                            details.push(format!("url: {}", u));
                                                        }
                                                        if let Some(p) = input.get("prompt").and_then(|v| v.as_str()) {
                                                            details.push(format!("prompt: {}", p.chars().take(200).collect::<String>()));
                                                        }
                                                        if !details.is_empty() {
                                                            info["detail"] = serde_json::json!(details.join(", "));
                                                        }
                                                        if let Some(result) = tool_results.get(id) {
                                                            info["result"] = result.clone();
                                                        }
                                                        Some(info)
                                                    }
                                                    _ => None,
                                                }
                                            }).collect();
                                            let mut entry = serde_json::json!({
                                                "role": "assistant",
                                                "text": text,
                                                "timestamp": m.timestamp,
                                                "turn": turn_num,
                                                "cost_usd": cost_usd,
                                                "duration_ms": duration_ms,
                                            });
                                            if !tools.is_empty() {
                                                entry["tools"] = serde_json::json!(tools);
                                            }
                                            if let Some(usage) = &message.usage {
                                                entry["usage"] = serde_json::json!(usage);
                                            }
                                            entry
                                        }
                                        _ => serde_json::json!({"role": "system", "timestamp": m.timestamp, "turn": turn_num}),
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
                                // Search across all sessions using CrossSessionDb (FTS5)
                                let mut results: Vec<serde_json::Value> = Vec::new();
                                match engine::cross_session_db::CrossSessionDb::new() {
                                    Ok(db) => {
                                        let hits = db.search_with_context(&query, max_results);
                                        for hit in hits {
                                            results.push(serde_json::json!({
                                                "snippet": hit.snippet,
                                                "timestamp": hit.timestamp,
                                                "session_id": hit.session_id,
                                                "cwd": hit.cwd,
                                                "rank": hit.rank,
                                            }));
                                        }
                                    }
                                    Err(e) => {
                                        eprintln!("CrossSessionDb error: {}, falling back to in-memory search", e);
                                        // Fallback: search current session only
                                        let engine = session.engine_read().await;
                                        let messages = engine.get_messages();
                                        let query_lower = query.to_lowercase();
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
                                    }
                                }

                                let mut conn_guard = conn.lock().await;
                                let _ = conn_guard.send_response(id, serde_json::json!({
                                    "results": results,
                                    "count": results.len(),
                                    "query": query,
                                })).await;
                            }
                            ClientMethod::DocUpload { file_path } => {
                                let path = std::path::Path::new(&file_path);
                                let mut conn_guard = conn.lock().await;
                                match doc_upload::build_attachment_from_file(path) {
                                    Ok(attachment) => {
                                        let _ = conn_guard.send_response(id, serde_json::json!({
                                            "attachment": attachment,
                                            "file_path": file_path,
                                        })).await;
                                    }
                                    Err(e) => {
                                        let _ = conn_guard.send_error(Some(id), -32000,
                                            format!("文档上传失败: {}", e)).await;
                                    }
                                }
                            }
                            ClientMethod::Export { output_path } => {
                                // Get conversation history (reuse TalkTail logic)
                                let engine = session.engine_read().await;
                                let messages = engine.get_messages();
                                let tail: Vec<serde_json::Value> = messages.iter().enumerate().map(|(idx, m)| {
                                    let turn_num = idx + 1;
                                    match &m.content {
                                        crate::models::message::MessageContent::User { message, tool_use_result, .. } => {
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
                                            let is_tool_result = tool_use_result.is_some();
                                            let mut entry = serde_json::json!({
                                                "role": "user",
                                                "text": text,
                                                "timestamp": m.timestamp,
                                                "turn": turn_num,
                                            });
                                            if is_tool_result {
                                                entry["is_tool_result"] = serde_json::json!(true);
                                            }
                                            entry
                                        }
                                        crate::models::message::MessageContent::Assistant { message, cost_usd, .. } => {
                                            let text: String = message.content.iter().filter_map(|b| {
                                                match b {
                                                    crate::models::message::ContentBlock::Text { text } => Some(text.clone()),
                                                    _ => None,
                                                }
                                            }).collect::<Vec<_>>().join("");
                                            let tools: Vec<serde_json::Value> = message.content.iter().filter_map(|b| {
                                                match b {
                                                    crate::models::message::ContentBlock::ToolUse { id, name, input } => {
                                                        let mut info = serde_json::json!({"name": name, "id": id});
                                                        let mut details: Vec<String> = Vec::new();
                                                        if let Some(cmd) = input.get("command").and_then(|v| v.as_str()) {
                                                            details.push(format!("command: {}", cmd));
                                                        }
                                                        if let Some(fp) = input.get("file_path").and_then(|v| v.as_str()) {
                                                            details.push(format!("path: {}", fp));
                                                        }
                                                        if !details.is_empty() {
                                                            info["detail"] = serde_json::json!(details.join(", "));
                                                        }
                                                        Some(info)
                                                    }
                                                    _ => None,
                                                }
                                            }).collect();
                                            let mut entry = serde_json::json!({
                                                "role": "assistant",
                                                "text": text,
                                                "timestamp": m.timestamp,
                                                "turn": turn_num,
                                                "cost_usd": cost_usd,
                                            });
                                            if !tools.is_empty() {
                                                entry["tools"] = serde_json::json!(tools);
                                            }
                                            entry
                                        }
                                        _ => serde_json::json!({"role": "system", "text": "", "timestamp": m.timestamp, "turn": turn_num}),
                                    }
                                }).collect();
                                drop(engine);

                                // Convert to ExportEntry and format
                                let export_entries: Vec<engine::export::ExportEntry> = tail.iter()
                                    .filter_map(|v| serde_json::from_value(v.clone()).ok())
                                    .collect();

                                if export_entries.is_empty() {
                                    let mut conn_guard = conn.lock().await;
                                    let _ = conn_guard.send_error(Some(id), -32000,
                                        "当前会话无对话记录".to_string()).await;
                                } else {
                                    let markdown = engine::export::format_transcript_to_markdown(&export_entries);
                                    let file_path = output_path.unwrap_or_else(|| {
                                        work_cwd.join(engine::export::default_export_filename())
                                            .to_string_lossy().to_string()
                                    });

                                    let mut conn_guard = conn.lock().await;
                                    match std::fs::write(&file_path, &markdown) {
                                        Ok(()) => {
                                            let _ = conn_guard.send_response(id, serde_json::json!({
                                                "file_path": file_path,
                                                "message_count": export_entries.len(),
                                                "size_bytes": markdown.len(),
                                            })).await;
                                        }
                                        Err(e) => {
                                            let _ = conn_guard.send_error(Some(id), -32000,
                                                format!("导出文件写入失败: {}", e)).await;
                                        }
                                    }
                                }
                            }

                            // ── Spec-Driven Development RPC ──
                            ClientMethod::SpecNew { feature_name, workflow, spec_type } => {
                                let spec_engine = engine::spec_engine::SpecEngine::new(work_cwd.clone());
                                let wf = match workflow.as_deref() {
                                    Some("design") => engine::spec_engine::SpecWorkflow::DesignFirst,
                                    _ => engine::spec_engine::SpecWorkflow::RequirementsFirst,
                                };
                                let st = match spec_type.as_deref() {
                                    Some("bugfix") => engine::spec_engine::SpecType::Bugfix,
                                    _ => engine::spec_engine::SpecType::Feature,
                                };
                                let mut conn_guard = conn.lock().await;
                                match spec_engine.create_spec(&feature_name, wf, st) {
                                    Ok(config) => {
                                        let _ = conn_guard.send_response(id, serde_json::json!({
                                            "status": "created",
                                            "feature_name": feature_name,
                                            "config": serde_json::to_value(&config).unwrap_or_default()
                                        })).await;
                                    }
                                    Err(e) => {
                                        let _ = conn_guard.send_error(Some(id), -32001, e.to_string()).await;
                                    }
                                }
                            }
                            ClientMethod::SpecList => {
                                let spec_engine = engine::spec_engine::SpecEngine::new(work_cwd.clone());
                                let mut conn_guard = conn.lock().await;
                                match spec_engine.list_specs() {
                                    Ok(specs) => {
                                        let _ = conn_guard.send_response(id, serde_json::json!({"specs": specs})).await;
                                    }
                                    Err(e) => {
                                        let _ = conn_guard.send_error(Some(id), -32000, e.to_string()).await;
                                    }
                                }
                            }
                            ClientMethod::SpecShow { feature_name } => {
                                let spec_engine = engine::spec_engine::SpecEngine::new(work_cwd.clone());
                                let mut conn_guard = conn.lock().await;
                                match spec_engine.get_spec(&feature_name) {
                                    Ok(summary) => {
                                        let _ = conn_guard.send_response(id, serde_json::to_value(&summary).unwrap_or_default()).await;
                                    }
                                    Err(e) => {
                                        let _ = conn_guard.send_error(Some(id), -32001, e.to_string()).await;
                                    }
                                }
                            }
                            ClientMethod::SpecStatus { feature_name } => {
                                let spec_engine = engine::spec_engine::SpecEngine::new(work_cwd.clone());
                                let mut conn_guard = conn.lock().await;
                                match spec_engine.get_status(&feature_name) {
                                    Ok(progress) => {
                                        let _ = conn_guard.send_response(id, serde_json::to_value(&progress).unwrap_or_default()).await;
                                    }
                                    Err(e) => {
                                        let _ = conn_guard.send_error(Some(id), -32001, e.to_string()).await;
                                    }
                                }
                            }
                            ClientMethod::SpecRun { feature_name, task_id } => {
                                let spec_engine = engine::spec_engine::SpecEngine::new(work_cwd.clone());
                                let mut conn_guard = conn.lock().await;
                                let task = if let Some(_tid) = &task_id {
                                    // Find specific task
                                    match spec_engine.next_task(&feature_name) {
                                        Ok(t) => t,
                                        Err(e) => {
                                            let _ = conn_guard.send_error(Some(id), -32001, e.to_string()).await;
                                            continue;
                                        }
                                    }
                                } else {
                                    match spec_engine.next_task(&feature_name) {
                                        Ok(t) => t,
                                        Err(e) => {
                                            let _ = conn_guard.send_error(Some(id), -32001, e.to_string()).await;
                                            continue;
                                        }
                                    }
                                };
                                match task {
                                    Some(t) => {
                                        let _ = conn_guard.send_response(id, serde_json::json!({
                                            "status": "ready",
                                            "task_id": t.id,
                                            "task_description": t.description,
                                        })).await;
                                    }
                                    None => {
                                        let _ = conn_guard.send_response(id, serde_json::json!({
                                            "status": "all_complete",
                                            "message": "All tasks are completed"
                                        })).await;
                                    }
                                }
                            }
                            ClientMethod::SpecEdit { feature_name, phase } => {
                                let spec_engine = engine::spec_engine::SpecEngine::new(work_cwd.clone());
                                let mut conn_guard = conn.lock().await;
                                match spec_engine.read_phase_doc(&feature_name, &phase) {
                                    Ok(content) => {
                                        let _ = conn_guard.send_response(id, serde_json::json!({
                                            "feature_name": feature_name,
                                            "phase": phase,
                                            "content": content,
                                        })).await;
                                    }
                                    Err(e) => {
                                        let _ = conn_guard.send_error(Some(id), -32001, e.to_string()).await;
                                    }
                                }
                            }
                            ClientMethod::HooksList => {
                                let hooks = shared.hook_manager.get_hooks().await;
                                let count = hooks.len();
                                let hooks_json: Vec<serde_json::Value> = hooks.iter().map(|h| {
                                    serde_json::to_value(h).unwrap_or_default()
                                }).collect();
                                let mut conn_guard = conn.lock().await;
                                let _ = conn_guard.send_response(id, serde_json::json!({
                                    "hooks": hooks_json,
                                    "count": count
                                })).await;
                            }
                            ClientMethod::HooksAdd { id: hook_id, name, trigger, filter, action, enabled, priority } => {
                                use std::str::FromStr;
                                use engine::hooks::{Hook, Action, TriggerType, Filter};

                                // Parse trigger type
                                let trigger_type = match TriggerType::from_str(&trigger) {
                                    Ok(t) => t,
                                    Err(e) => {
                                        let mut conn_guard = conn.lock().await;
                                        let _ = conn_guard.send_error(Some(id), -32602, e).await;
                                        continue;
                                    }
                                };

                                // Parse action
                                let hook_action: Action = match serde_json::from_value(action) {
                                    Ok(a) => a,
                                    Err(e) => {
                                        let mut conn_guard = conn.lock().await;
                                        let _ = conn_guard.send_error(Some(id), -32602, format!("Invalid action: {}", e)).await;
                                        continue;
                                    }
                                };

                                // Parse filter if provided
                                let hook_filter: Option<Filter> = match filter {
                                    Some(f) => match serde_json::from_value(f) {
                                        Ok(f) => Some(f),
                                        Err(e) => {
                                            let mut conn_guard = conn.lock().await;
                                            let _ = conn_guard.send_error(Some(id), -32602, format!("Invalid filter: {}", e)).await;
                                            continue;
                                        }
                                    },
                                    None => None,
                                };

                                // Create the hook
                                let mut hook = Hook::new(hook_id.clone(), name, trigger_type, hook_action);
                                hook.enabled = enabled;
                                hook.priority = priority;
                                if let Some(f) = hook_filter {
                                    hook.filter = Some(f);
                                }

                                // Add the hook
                                match shared.hook_manager.add_hook(hook.clone()).await {
                                    Ok(()) => {
                                        let mut conn_guard = conn.lock().await;
                                        let _ = conn_guard.send_response(id, serde_json::json!({
                                            "hook": hook,
                                            "message": "Hook added successfully"
                                        })).await;
                                    }
                                    Err(e) => {
                                        let mut conn_guard = conn.lock().await;
                                        let _ = conn_guard.send_error(Some(id), -32000, e).await;
                                    }
                                }
                            }
                            ClientMethod::HooksToggle { id: hook_id } => {
                                match shared.hook_manager.toggle_hook(&hook_id).await {
                                    Some(new_enabled) => {
                                        let mut conn_guard = conn.lock().await;
                                        let _ = conn_guard.send_response(id, serde_json::json!({
                                            "id": hook_id,
                                            "enabled": new_enabled,
                                            "message": if new_enabled { "Hook enabled" } else { "Hook disabled" }
                                        })).await;
                                    }
                                    None => {
                                        let mut conn_guard = conn.lock().await;
                                        let _ = conn_guard.send_error(Some(id), -32000, format!("Hook not found: {}", hook_id)).await;
                                    }
                                }
                            }
                            ClientMethod::HooksRemove { id: hook_id } => {
                                let removed = shared.hook_manager.remove_hook(&hook_id).await;
                                let mut conn_guard = conn.lock().await;
                                let _ = conn_guard.send_response(id, serde_json::json!({
                                    "id": hook_id,
                                    "removed": removed,
                                    "message": if removed { "Hook removed" } else { "Hook not found" }
                                })).await;
                            }

                            // ── Team Management RPC ──
                            ClientMethod::TeamSpawn { count, mode, task, policy } => {
                                use engine::team::{TeamExecutor, TeamConfig, TeamMode as EngineTeamMode, TeamPolicy};
                                use std::str::FromStr;

                                // Parse mode
                                let team_mode = match EngineTeamMode::from_str(&mode) {
                                    Ok(m) => m,
                                    Err(e) => {
                                        let mut conn_guard = conn.lock().await;
                                        let _ = conn_guard.send_error(Some(id), -32602, e).await;
                                        continue;
                                    }
                                };

                                // Parse policy if provided
                                let team_policy: Option<TeamPolicy> = match policy {
                                    Some(p) => match serde_json::from_value(p) {
                                        Ok(policy) => Some(policy),
                                        Err(e) => {
                                            let mut conn_guard = conn.lock().await;
                                            let _ = conn_guard.send_error(Some(id), -32602, format!("Invalid policy: {}", e)).await;
                                            continue;
                                        }
                                    },
                                    None => None,
                                };

                                // Create team config
                                let config = TeamConfig {
                                    mode: team_mode.clone(),
                                    policy: team_policy,
                                    cwd: Some(work_cwd.to_string_lossy().to_string()),
                                    model: Some(shared.state_manager.get().model.clone()),
                                    ..Default::default()
                                };

                                // Create the executor and team
                                let executor = TeamExecutor::new(
                                    Arc::clone(&shared.api_client),
                                    shared.engine_tools.clone(),
                                    work_cwd.clone(),
                                    shared.state_manager.get().model.clone(),
                                );

                                match executor.create_team(task.clone(), config).await {
                                    Ok(mut team) => {
                                        // For parallel mode, create the specified number of agents
                                        if team_mode == EngineTeamMode::Parallel {
                                            if let Err(e) = executor.add_parallel_agents(&mut team, count.unwrap_or(1), &task).await {
                                                let mut conn_guard = conn.lock().await;
                                                let _ = conn_guard.send_error(Some(id), -32000, format!("Failed to add agents: {}", e.message)).await;
                                                continue;
                                            }
                                        }

                                        let team_id = team.id.clone();
                                        let team_json = serde_json::to_value(&team).unwrap_or_default();

                                        // Store the team
                                        shared.team_executor.store_team(team).await;

                                        let mut conn_guard = conn.lock().await;
                                        let _ = conn_guard.send_response(id, serde_json::json!({
                                            "team_id": team_id,
                                            "team": team_json,
                                            "message": "Team created successfully"
                                        })).await;
                                    }
                                    Err(e) => {
                                        let mut conn_guard = conn.lock().await;
                                        let _ = conn_guard.send_error(Some(id), -32000, e.message).await;
                                    }
                                }
                            }
                            ClientMethod::TeamList => {
                                let teams = shared.team_executor.list_teams().await;
                                let count = teams.len();
                                let mut conn_guard = conn.lock().await;
                                let _ = conn_guard.send_response(id, serde_json::json!({
                                    "teams": teams,
                                    "count": count
                                })).await;
                            }
                            ClientMethod::TeamStatus { team_id } => {
                                match shared.team_executor.get_team(&team_id).await {
                                    Some(team) => {
                                        let summary = team.summary();
                                        let mut conn_guard = conn.lock().await;
                                        let _ = conn_guard.send_response(id, serde_json::json!({
                                            "team": team,
                                            "summary": summary
                                        })).await;
                                    }
                                    None => {
                                        let mut conn_guard = conn.lock().await;
                                        let _ = conn_guard.send_error(Some(id), -32000, format!("Team not found: {}", team_id)).await;
                                    }
                                }
                            }
                            ClientMethod::TeamResults { team_id } => {
                                match shared.team_executor.get_team(&team_id).await {
                                    Some(team) => {
                                        let results = team.collect_results();
                                        let agents: Vec<serde_json::Value> = team.agents.iter().map(|a| {
                                            serde_json::json!({
                                                "id": a.id,
                                                "status": a.status.to_string(),
                                                "result": a.result,
                                                "error": a.error,
                                                "tokens_used": a.tokens_used,
                                                "cost_usd": a.cost_usd,
                                            })
                                        }).collect();
                                        let mut conn_guard = conn.lock().await;
                                        let _ = conn_guard.send_response(id, serde_json::json!({
                                            "team_id": team_id,
                                            "status": team.status.to_string(),
                                            "results": results,
                                            "agents": agents,
                                            "total_tokens": team.total_tokens,
                                            "total_cost_usd": team.total_cost_usd,
                                        })).await;
                                    }
                                    None => {
                                        let mut conn_guard = conn.lock().await;
                                        let _ = conn_guard.send_error(Some(id), -32000, format!("Team not found: {}", team_id)).await;
                                    }
                                }
                            }
                            ClientMethod::TeamAbort { team_id } => {
                                match shared.team_executor.abort_team(&team_id).await {
                                    Some(team) => {
                                        let mut conn_guard = conn.lock().await;
                                        let _ = conn_guard.send_response(id, serde_json::json!({
                                            "team_id": team_id,
                                            "status": team.status.to_string(),
                                            "message": "Team aborted"
                                        })).await;
                                    }
                                    None => {
                                        let mut conn_guard = conn.lock().await;
                                        let _ = conn_guard.send_error(Some(id), -32000, format!("Team not found: {}", team_id)).await;
                                    }
                                }
                            }
                            ClientMethod::TeamExecute { team_id } => {
                                match shared.team_executor.get_team(&team_id).await {
                                    Some(team) => {
                                        // Spawn execution in background
                                        let executor = engine::team::TeamExecutor::new(
                                            Arc::clone(&shared.api_client),
                                            shared.engine_tools.clone(),
                                            work_cwd.clone(),
                                            shared.state_manager.get().model.clone(),
                                        );

                                        // Execute the team
                                        let result = executor.execute(team).await;

                                        let mut conn_guard = conn.lock().await;
                                        let _ = conn_guard.send_response(id, serde_json::json!({
                                            "team_id": result.team.id,
                                            "success": result.success,
                                            "error": result.error,
                                            "duration_ms": result.duration_ms,
                                            "status": result.team.status.to_string(),
                                            "total_tokens": result.team.total_tokens,
                                            "total_cost_usd": result.team.total_cost_usd,
                                        })).await;
                                    }
                                    None => {
                                        let mut conn_guard = conn.lock().await;
                                        let _ = conn_guard.send_error(Some(id), -32000, format!("Team not found: {}", team_id)).await;
                                    }
                                }
                            }

                            // ── Template Engine handlers ──
                            ClientMethod::TemplateList => {
                                let engine = engine::template::engine::TemplateEngine::new();
                                let templates = engine.list_all();
                                let result: Vec<serde_json::Value> = templates.iter().map(|t| {
                                    serde_json::json!({
                                        "name": t.name,
                                        "trigger": t.trigger,
                                        "description": t.description,
                                        "version": t.version,
                                        "author": t.author,
                                        "builtin": t.builtin,
                                        "tags": t.tags,
                                        "variables_count": t.variables.len(),
                                        "steps_count": t.workflow.len(),
                                    })
                                }).collect();
                                let mut conn_guard = conn.lock().await;
                                let _ = conn_guard.send_response(id, serde_json::json!({"templates": result, "count": result.len()})).await;
                            }
                            ClientMethod::TemplateCreate { json } => {
                                let mut engine = engine::template::engine::TemplateEngine::new();
                                match serde_json::from_str::<engine::template::types::Template>(&json) {
                                    Ok(template) => {
                                        match engine.create_template(&template) {
                                            Ok(()) => {
                                                let mut conn_guard = conn.lock().await;
                                                let _ = conn_guard.send_response(id, serde_json::json!({"success": true, "name": template.name})).await;
                                            }
                                            Err(e) => {
                                                let mut conn_guard = conn.lock().await;
                                                let _ = conn_guard.send_error(Some(id), -32000, format!("Failed to create template: {}", e)).await;
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        let mut conn_guard = conn.lock().await;
                                        let _ = conn_guard.send_error(Some(id), -32000, format!("Invalid template JSON: {}", e)).await;
                                    }
                                }
                            }
                            ClientMethod::TemplateDelete { name } => {
                                let mut engine = engine::template::engine::TemplateEngine::new();
                                match engine.delete_template(&name) {
                                    Ok(()) => {
                                        let mut conn_guard = conn.lock().await;
                                        let _ = conn_guard.send_response(id, serde_json::json!({"success": true, "name": name})).await;
                                    }
                                    Err(e) => {
                                        let mut conn_guard = conn.lock().await;
                                        let _ = conn_guard.send_error(Some(id), -32000, format!("Failed to delete template: {}", e)).await;
                                    }
                                }
                            }
                            ClientMethod::TemplateExport { name } => {
                                let engine = engine::template::engine::TemplateEngine::new();
                                match engine.export_template(&name) {
                                    Ok(json_str) => {
                                        let mut conn_guard = conn.lock().await;
                                        let _ = conn_guard.send_response(id, serde_json::json!({"name": name, "template": serde_json::from_str::<serde_json::Value>(&json_str).unwrap_or_default()})).await;
                                    }
                                    Err(e) => {
                                        let mut conn_guard = conn.lock().await;
                                        let _ = conn_guard.send_error(Some(id), -32000, format!("Failed to export template: {}", e)).await;
                                    }
                                }
                            }
                            ClientMethod::TemplateImport { url } => {
                                let mut engine = engine::template::engine::TemplateEngine::new();
                                match engine.import_template_url(&url).await {
                                    Ok(template) => {
                                        let mut conn_guard = conn.lock().await;
                                        let _ = conn_guard.send_response(id, serde_json::json!({"success": true, "name": template.name, "trigger": template.trigger})).await;
                                    }
                                    Err(e) => {
                                        let mut conn_guard = conn.lock().await;
                                        let _ = conn_guard.send_error(Some(id), -32000, format!("Failed to import template: {}", e)).await;
                                    }
                                }
                            }

                            // ── Git Integration handlers ──
                            ClientMethod::GitPrList => {
                                let result = match engine::git_integration::pr::PrManager::list_prs(None).await {
                                    Ok(prs) => {
                                        let pr_list: Vec<serde_json::Value> = prs.iter().map(|p| {
                                            serde_json::json!({
                                                "number": p.number,
                                                "title": p.title,
                                                "state": p.state,
                                                "author": p.author,
                                                "base_branch": p.base_branch,
                                                "head_branch": p.head_branch,
                                                "created_at": p.created_at,
                                                "url": p.url,
                                            })
                                        }).collect();
                                        serde_json::json!({"pull_requests": pr_list, "count": pr_list.len()})
                                    }
                                    Err(e) => serde_json::json!({"error": format!("{}", e)}),
                                };
                                let mut conn_guard = conn.lock().await;
                                let _ = conn_guard.send_response(id, result).await;
                            }
                            ClientMethod::GitPrCreate { title, body, base, head } => {
                                let body_opt = if body.is_empty() { None } else { Some(body.as_str()) };
                                let base_opt = if base.is_empty() { None } else { Some(base.as_str()) };
                                let result = match engine::git_integration::pr::PrManager::create_pr(&title, body_opt, base_opt).await {
                                    Ok(pr) => serde_json::json!({
                                        "success": true,
                                        "number": pr.number,
                                        "title": pr.title,
                                        "url": pr.url,
                                    }),
                                    Err(e) => serde_json::json!({"success": false, "error": format!("{}", e)}),
                                };
                                let mut conn_guard = conn.lock().await;
                                let _ = conn_guard.send_response(id, result).await;
                            }
                            ClientMethod::GitBranchList => {
                                let result = match engine::git_integration::branch::BranchManager::list_branches().await {
                                    Ok(branches) => {
                                        let branch_list: Vec<serde_json::Value> = branches.iter().map(|b| {
                                            serde_json::json!({
                                                "name": b.name,
                                                "is_current": b.is_current,
                                                "ahead": b.ahead,
                                                "behind": b.behind,
                                                "last_commit": b.last_commit,
                                                "last_commit_msg": b.last_commit_msg,
                                            })
                                        }).collect();
                                        serde_json::json!({"branches": branch_list, "count": branch_list.len()})
                                    }
                                    Err(e) => serde_json::json!({"error": format!("{}", e)}),
                                };
                                let mut conn_guard = conn.lock().await;
                                let _ = conn_guard.send_response(id, result).await;
                            }
                            ClientMethod::GitBranchCreate { name } => {
                                let result = match engine::git_integration::branch::BranchManager::create_branch(&name, None).await {
                                    Ok(()) => serde_json::json!({"success": true, "name": name}),
                                    Err(e) => serde_json::json!({"success": false, "error": format!("{}", e)}),
                                };
                                let mut conn_guard = conn.lock().await;
                                let _ = conn_guard.send_response(id, result).await;
                            }
                            ClientMethod::GitConflictCheck => {
                                let result = match engine::git_integration::conflict::ConflictResolver::detect_conflicts().await {
                                    Ok(conflicts) => {
                                        let conflict_list: Vec<serde_json::Value> = conflicts.iter().map(|c| {
                                            serde_json::json!({
                                                "file": c.file,
                                                "resolved": c.resolved,
                                            })
                                        }).collect();
                                        serde_json::json!({"conflicts": conflict_list, "count": conflict_list.len(), "has_conflicts": !conflicts.is_empty()})
                                    }
                                    Err(e) => serde_json::json!({"error": format!("{}", e)}),
                                };
                                let mut conn_guard = conn.lock().await;
                                let _ = conn_guard.send_response(id, result).await;
                            }
                            ClientMethod::GitCommitAmend => {
                                let result = match engine::git_integration::commit::CommitManager::amend_commit().await {
                                    Ok(()) => serde_json::json!({"success": true}),
                                    Err(e) => serde_json::json!({"success": false, "error": format!("{}", e)}),
                                };
                                let mut conn_guard = conn.lock().await;
                                let _ = conn_guard.send_response(id, result).await;
                            }
                            ClientMethod::GitUndo => {
                                let result = match engine::git_integration::commit::CommitManager::undo_commit().await {
                                    Ok(()) => serde_json::json!({"success": true}),
                                    Err(e) => serde_json::json!({"success": false, "error": format!("{}", e)}),
                                };
                                let mut conn_guard = conn.lock().await;
                                let _ = conn_guard.send_response(id, result).await;
                            }

                            // ── Model Router handlers ──
                            ClientMethod::ModelList => {
                                let router = engine::model_router::router::ModelRouter::new();
                                let models = router.list_models();
                                let model_list: Vec<serde_json::Value> = models.iter().map(|m| {
                                    serde_json::json!({
                                        "name": m.name,
                                        "provider": m.provider,
                                        "max_tokens": m.max_tokens,
                                        "cost_per_1k_input": m.cost_per_1k_input,
                                        "cost_per_1k_output": m.cost_per_1k_output,
                                        "capabilities": m.capabilities,
                                        "priority": m.priority,
                                    })
                                }).collect();
                                let mut conn_guard = conn.lock().await;
                                let _ = conn_guard.send_response(id, serde_json::json!({"models": model_list, "count": model_list.len()})).await;
                            }
                            ClientMethod::ModelRoute { task } => {
                                let router = engine::model_router::router::ModelRouter::new();
                                let decision = router.route(&task, 0, 0.5);
                                let mut conn_guard = conn.lock().await;
                                let _ = conn_guard.send_response(id, serde_json::json!({
                                    "selected_model": decision.selected_model,
                                    "reason": decision.reason,
                                    "confidence": decision.confidence,
                                })).await;
                            }
                            ClientMethod::ModelBudget => {
                                let mut budget = engine::model_router::budget::BudgetManager::load();
                                let result = serde_json::json!({
                                    "daily_limit": budget.daily_limit,
                                    "monthly_limit": budget.monthly_limit,
                                    "current_daily": budget.current_daily,
                                    "current_monthly": budget.current_monthly,
                                    "remaining_daily": budget.remaining_daily(),
                                    "remaining_monthly": budget.remaining_monthly(),
                                });
                                let mut conn_guard = conn.lock().await;
                                let _ = conn_guard.send_response(id, result).await;
                            }
                            ClientMethod::ModelStats => {
                                let router = engine::model_router::router::ModelRouter::new();
                                let models = router.list_models();
                                let rules = router.list_rules();
                                let result = serde_json::json!({
                                    "models_count": models.len(),
                                    "rules_count": rules.len(),
                                    "rules": rules.iter().map(|r| serde_json::json!({
                                        "id": r.id,
                                        "description": r.description,
                                        "target_model": r.target_model,
                                        "priority": r.priority,
                                        "enabled": r.enabled,
                                    })).collect::<Vec<_>>(),
                                });
                                let mut conn_guard = conn.lock().await;
                                let _ = conn_guard.send_response(id, result).await;
                            }

                            // ── Telemetry handlers ──
                            ClientMethod::TelemetryStats => {
                                let collector = match engine::telemetry::collector::TelemetryCollector::new() {
                                    Ok(c) => c,
                                    Err(e) => {
                                        let mut conn_guard = conn.lock().await;
                                        let _ = conn_guard.send_error(Some(id), -32000, format!("Telemetry unavailable: {}", e)).await;
                                        continue;
                                    }
                                };
                                match collector.get_stats() {
                                    Ok(stats) => {
                                        let mut conn_guard = conn.lock().await;
                                        let _ = conn_guard.send_response(id, serde_json::json!({
                                            "total_turns": stats.total_turns,
                                            "total_tokens": stats.total_tokens,
                                            "total_cost_usd": stats.total_cost_usd,
                                            "total_tools_called": stats.total_tools_called,
                                            "sessions_count": stats.sessions_count,
                                            "files_modified": stats.files_modified,
                                            "avg_response_time_ms": stats.avg_response_time_ms,
                                            "most_used_tool": stats.most_used_tool,
                                        })).await;
                                    }
                                    Err(e) => {
                                        let mut conn_guard = conn.lock().await;
                                        let _ = conn_guard.send_error(Some(id), -32000, format!("Failed to get stats: {}", e)).await;
                                    }
                                }
                            }
                            ClientMethod::TelemetryTrends { days } => {
                                let collector = match engine::telemetry::collector::TelemetryCollector::new() {
                                    Ok(c) => c,
                                    Err(e) => {
                                        let mut conn_guard = conn.lock().await;
                                        let _ = conn_guard.send_error(Some(id), -32000, format!("Telemetry unavailable: {}", e)).await;
                                        continue;
                                    }
                                };
                                let daily = match collector.get_daily_stats(days) {
                                    Ok(d) => d,
                                    Err(e) => {
                                        let mut conn_guard = conn.lock().await;
                                        let _ = conn_guard.send_error(Some(id), -32000, format!("Failed to get trends: {}", e)).await;
                                        continue;
                                    }
                                };
                                let daily_list: Vec<serde_json::Value> = daily.iter().map(|d| {
                                    serde_json::json!({
                                        "date": d.date,
                                        "turns": d.turns,
                                        "tokens": d.tokens,
                                        "cost": d.cost,
                                        "tools": d.tools,
                                        "sessions": d.sessions,
                                    })
                                }).collect();
                                let mut conn_guard = conn.lock().await;
                                let _ = conn_guard.send_response(id, serde_json::json!({"days": days, "daily": daily_list, "count": daily_list.len()})).await;
                            }
                            ClientMethod::TelemetryExport { format } => {
                                let collector = match engine::telemetry::collector::TelemetryCollector::new() {
                                    Ok(c) => c,
                                    Err(e) => {
                                        let mut conn_guard = conn.lock().await;
                                        let _ = conn_guard.send_error(Some(id), -32000, format!("Telemetry unavailable: {}", e)).await;
                                        continue;
                                    }
                                };
                                let exporter = engine::telemetry::export::TelemetryExporter::new(collector);
                                let result = match format.to_lowercase().as_str() {
                                    "json" => exporter.export_json(None),
                                    "csv" => exporter.export_csv(None),
                                    "summary" | "md" | "markdown" => exporter.export_summary(),
                                    _ => Err(format!("Unknown export format: {}. Use json, csv, or summary.", format)),
                                };
                                match result {
                                    Ok(data) => {
                                        let mut conn_guard = conn.lock().await;
                                        let _ = conn_guard.send_response(id, serde_json::json!({"format": format, "data": data})).await;
                                    }
                                    Err(e) => {
                                        let mut conn_guard = conn.lock().await;
                                        let _ = conn_guard.send_error(Some(id), -32000, format!("Export failed: {}", e)).await;
                                    }
                                }
                            }

                            // ── Permission Gate handlers ──
                            ClientMethod::PermissionStatus => {
                                let gate = engine::permission_gate::gate::RuleBasedPermissionGate::new();
                                let rules = gate.list_rules();
                                let rule_list: Vec<serde_json::Value> = rules.iter().map(|r| {
                                    serde_json::json!({
                                        "id": r.id,
                                        "description": r.description,
                                        "tool": r.tool,
                                        "action": r.action,
                                        "target_pattern": r.target_pattern,
                                        "require_confirmation": r.require_confirmation,
                                        "auto_deny": r.auto_deny,
                                    })
                                }).collect();
                                let mut conn_guard = conn.lock().await;
                                let _ = conn_guard.send_response(id, serde_json::json!({"rules": rule_list, "count": rule_list.len()})).await;
                            }
                            ClientMethod::PermissionGrant { tool, action, target, permanent } => {
                                let mut gate = engine::permission_gate::gate::RuleBasedPermissionGate::new();
                                let decision = if permanent {
                                    engine::permission_gate::types::DecisionType::AllowPermanent
                                } else {
                                    engine::permission_gate::types::DecisionType::AllowSession
                                };
                                gate.grant(&tool, &action, &target, decision, None);
                                let mut conn_guard = conn.lock().await;
                                let _ = conn_guard.send_response(id, serde_json::json!({"success": true, "tool": tool, "action": action, "target": target, "permanent": permanent})).await;
                            }
                            ClientMethod::PermissionRevoke { tool, action, target } => {
                                let mut gate = engine::permission_gate::gate::RuleBasedPermissionGate::new();
                                let removed = gate.revoke(&tool, &action, &target);
                                let mut conn_guard = conn.lock().await;
                                let _ = conn_guard.send_response(id, serde_json::json!({"success": removed > 0, "removed": removed})).await;
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

    // Cancel the broadcast receiver tasks
    broadcast_handle.abort();
    cron_broadcast_handle.abort();
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
                    context_window: shared_clone.baoclaw_config.context_window,
                    auto_compact_threshold_ratio: shared_clone.baoclaw_config.auto_compact_threshold_ratio,
                    parent_turn_id: None,
                    agent_label: None,
                    session_memory: Some(Arc::new(
                        crate::engine::session_memory::SessionMemory::load(&session_id_clone)
                    )),
                    file_cache: Some(Arc::clone(&shared_clone.file_cache)),
                    tool_result_store: shared_clone.tool_result_store.as_ref().map(Arc::clone),
                    hook_manager: Some(Arc::clone(&shared_clone.hook_manager)),
                })
            },
        ).await;

        // Auto-register this project in the registry
        shared.project_registry.ensure_registered(
            &work_cwd.to_string_lossy(), None
        ).await;

        // ── Resume session history: summary-first strategy ──
        // Inspired by Claude Code: load pre-written summary + recent tail,
        // NEVER rebuild the full history or do on-demand API summarization.
        let current_msg_count = session.engine_read().await.get_messages().len();
        if is_new || current_msg_count == 0 {
            let cwd_str_for_resume = work_cwd.to_string_lossy().to_string();
            if let Some(rid) = engine::transcript::find_latest_session_for_cwd(&cwd_str_for_resume) {
                match engine::transcript::TranscriptWriter::load(&rid) {
                    Ok(entries) => {
                        let entry_count = entries.len();
                        let old_summary_obj = crate::engine::session_memory::SessionMemory::load(&rid);
                        let old_summary = old_summary_obj.get();
                        let has_summary = old_summary_obj.is_available();

                        // ── Three-tier loading strategy ──
                        let messages = if has_summary {
                            // Tier 1 (best): pre-written summary exists
                            // Load summary + last 200 entries only — instant
                            let tail_size = 200.min(entry_count);
                            eprintln!("Session resume: loading pre-written summary + {} recent entries (of {} total)",
                                tail_size, entry_count);
                            engine::transcript::rebuild_messages_from_transcript_limited(
                                &entries, tail_size, Some(&old_summary),
                            )
                        } else if entry_count <= 400 {
                            // Tier 2: small session, no summary — safe to rebuild all
                            eprintln!("Session resume: small session ({} entries), rebuilding all", entry_count);
                            engine::transcript::rebuild_messages_from_transcript(&entries)
                        } else {
                            // Tier 3 (fallback): large session with NO summary
                            // Don't rebuild all (would cause 10-min auto-compact).
                            // Load last 200 entries with a warning header instead.
                            let tail_size = 200.min(entry_count);
                            eprintln!(
                                "WARNING: Large session ({} entries) with no pre-written summary. \
                                 Loading only last {} entries. Context from earlier turns may be lost. \
                                 (Summary will be generated during this session for next time.)",
                                entry_count, tail_size
                            );
                            let tail_entries = &entries[entry_count - tail_size..];
                            let mut msgs = engine::transcript::rebuild_messages_from_transcript(tail_entries);

                            // Prepend a warning so the LLM knows context is incomplete
                            if !msgs.is_empty() {
                                let warning = crate::models::message::Message {
                                    uuid: uuid::Uuid::new_v4().to_string(),
                                    timestamp: chrono::Utc::now().to_rfc3339(),
                                    content: crate::models::message::MessageContent::System {
                                        subtype: crate::models::message::SystemSubtype::CompactBoundary,
                                        content: format!(
                                            "[Session resumed — {} earlier conversation entries were omitted because no summary was available. \
                                             The current session will generate one for next time.]",
                                            entry_count - tail_size
                                        ),
                                    },
                                };
                                msgs.insert(0, warning);
                            }
                            msgs
                        };

                        if !messages.is_empty() {
                            let mut engine = session.engine_write().await;
                            engine.set_messages(messages);

                            // Load and apply persisted token baseline
                            engine.load_token_baseline(&rid).await;

                            // Seed the new session's memory with the old summary
                            engine.seed_session_memory(&old_summary);

                            eprintln!("Resumed session {} ({} entries → {} messages)",
                                rid, entry_count, engine.get_messages().len());
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
        let hook_cwd = work_cwd.to_string_lossy().to_string();
        handle_shared_client(conn, shared, session.clone(), client_id, broadcast_rx, work_cwd, session_id_clone.clone()).await;

        // Client disconnect handling (Task 6.1)
        let is_last = session.remove_client(client_id).await;
        if is_last {
            // ── Session-close evolution hook ──
            // Extract structured summary before removing the session.
            {
                let engine = session.engine_read().await;
                let messages = engine.get_messages();
                let usage = engine.get_usage().clone();
                let model = engine.get_model().to_string();
                let messages_clone = messages.to_vec();
                drop(engine);

                // Estimate session duration from first and last message timestamps
                let duration_secs = if messages_clone.len() >= 2 {
                    let first_ts = &messages_clone.first().unwrap().timestamp;
                    let last_ts = &messages_clone.last().unwrap().timestamp;
                    (|| -> Option<u64> {
                        let t1 = chrono::DateTime::parse_from_rfc3339(first_ts).ok()?;
                        let t2 = chrono::DateTime::parse_from_rfc3339(last_ts).ok()?;
                        Some((t2 - t1).num_seconds().max(0) as u64)
                    })().unwrap_or(0)
                } else {
                    0
                };

                // Estimate total cost from token usage (Claude Sonnet pricing)
                let estimated_cost = (usage.input_tokens as f64 * 3.0e-6)
                    + (usage.output_tokens as f64 * 15.0e-6)
                    + (usage.cache_read_input_tokens.unwrap_or(0) as f64 * 0.3e-6);

                // ── Save session memory on close if not yet written ──
                // This ensures that even if the background updater never ran
                // (e.g., short session), the next startup will have a summary.
                {
                    let engine = session.engine_read().await;
                    if let Some(ref sm) = engine.get_session_memory() {
                        if !sm.is_available() && messages_clone.len() >= 4 {
                            eprintln!("Session close: generating heuristic session memory ({} messages)",
                                messages_clone.len());

                            let mut summary_parts = vec![
                                "# Session Summary".to_string(),
                            ];

                            // Extract user messages as task list
                            let mut task_descriptions = Vec::new();
                            for msg in &messages_clone {
                                if let crate::models::message::MessageContent::User { message, .. } = &msg.content {
                                    if let serde_json::Value::String(s) = &message.content {
                                        let first_line = s.lines().next().unwrap_or("");
                                        if !first_line.is_empty() && first_line.len() < 200 {
                                            task_descriptions.push(first_line.to_string());
                                        }
                                    }
                                }
                            }

                            if !task_descriptions.is_empty() {
                                summary_parts.push("## Tasks Discussed".to_string());
                                for (i, task) in task_descriptions.iter().take(20).enumerate() {
                                    summary_parts.push(format!("{}. {}", i + 1, task));
                                }
                            }

                            summary_parts.push(format!("\n## Stats\n- Messages: {}\n- Duration: {}s\n- Cost: ${:.4}",
                                messages_clone.len(), duration_secs, estimated_cost));

                            let summary = summary_parts.join("\n");
                            sm.update(summary);
                            eprintln!("Session memory saved on close ({} chars)", sm.get().len());
                        }
                    }
                }

                shared_clone.evolution_engine.on_session_close(
                    &session_id_clone,
                    &hook_cwd,
                    &model,
                    &messages_clone,
                    &usage,
                    estimated_cost,
                    duration_secs,
                ).await;
            }

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

    // Parse --sandbox flag for sandboxed command execution
    // Usage: --sandbox bwrap | --sandbox docker | --sandbox none
    // If flag is omitted, no sandbox is used (direct execution).
    let sandbox_config: Option<Arc<engine::sandbox::SandboxConfig>> =
        args.iter().position(|a| a == "--sandbox")
        .and_then(|i| args.get(i + 1))
        .map(|s| s.as_str())
        .map(|mode| {
            use engine::sandbox::{SandboxBackend, SandboxConfig};
            let backend = match mode {
                "bwrap" | "bubblewrap" => {
                    eprintln!("[sandbox] Using Bubblewrap (bwrap) isolation");
                    SandboxBackend::Bubblewrap
                }
                "docker" => {
                    eprintln!("[sandbox] Using Docker container isolation");
                    SandboxBackend::Docker {
                        image: std::env::var("BAOCLAW_SANDBOX_IMAGE")
                            .unwrap_or_else(|_| "baoclaw-sandbox:latest".into()),
                    }
                }
                "none" | "off" => {
                    eprintln!("[sandbox] Sandbox disabled (direct execution)");
                    SandboxBackend::None
                }
                other => {
                    eprintln!("[sandbox] WARNING: unknown sandbox mode '{}', falling back to auto-detect", other);
                    SandboxConfig::auto_detect().backend
                }
            };
            let mut cfg = SandboxConfig {
                backend,
                ..SandboxConfig::default()
            };
            // Auto-mount the working directory as read-write
            cfg.rw_mounts.push(cwd_str.clone());
            // Mount ~/.baoclaw for config/memory/session data access
            let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
            let baoclaw_dir = format!("{}/.baoclaw", home);
            if std::path::Path::new(&baoclaw_dir).exists() {
                cfg.rw_mounts.push(baoclaw_dir);
            }
            // Mount /tmp for temp file exchange between host and sandbox
            cfg.rw_mounts.push("/tmp".to_string());
            // Set workdir to the project CWD
            cfg.workdir = Some(cwd_str.clone());
            Arc::new(cfg)
        });

    // If --sandbox flag was provided without a value, use auto-detect
    let sandbox_config = sandbox_config.or_else(|| {
        if args.iter().any(|a| a == "--sandbox") {
            eprintln!("[sandbox] No mode specified, auto-detecting...");
            let mut cfg = engine::sandbox::SandboxConfig::auto_detect();
            cfg.rw_mounts.push(cwd_str.clone());
            let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
            let baoclaw_dir = format!("{}/.baoclaw", home);
            if std::path::Path::new(&baoclaw_dir).exists() {
                cfg.rw_mounts.push(baoclaw_dir);
            }
            cfg.rw_mounts.push("/tmp".to_string());
            cfg.workdir = Some(cwd_str.clone());
            eprintln!("[sandbox] Auto-detected: {}", cfg.description());
            Some(Arc::new(cfg))
        } else {
            None
        }
    });

    // Validate sandbox configuration at startup
    if let Some(ref cfg) = sandbox_config {
        if let Some(err) = cfg.validate() {
            eprintln!("[sandbox] ERROR: {}", err);
            eprintln!("[sandbox] Falling back to direct execution (no sandbox)");
        } else {
            eprintln!("[sandbox] ✓ Sandbox ready: {}", cfg.description());
        }
    }

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

    // === P1-1: Model profiles support ===
    // Resolve the primary profile (auto-migrated from old format by normalize_profiles).
    // If model_profiles is populated, use the primary profile's api_type/key/base_url.
    // Otherwise, fall back to the old env-var-based logic for backward compatibility.
    //
    // resolve_api_key priority: profile.api_key → env var based on api_type
    // resolve_base_url priority: profile.base_url → env var based on api_type
    fn resolve_api_key(profile: &config::ModelProfile) -> String {
        // 1. Prefer profile.api_key (new format)
        if let Some(key) = &profile.api_key {
            if !key.is_empty() {
                return key.clone();
            }
        }
        // 2. Fall back to environment variable (backward compat)
        match profile.api_type.as_str() {
            "openai" => std::env::var("OPENAI_API_KEY").unwrap_or_default(),
            _ => std::env::var("ANTHROPIC_API_KEY").unwrap_or_default(),
        }
    }
    fn resolve_base_url(profile: &config::ModelProfile) -> Option<String> {
        if let Some(url) = &profile.base_url {
            if !url.is_empty() {
                return Some(url.clone());
            }
        }
        match profile.api_type.as_str() {
            "openai" => std::env::var("OPENAI_BASE_URL").ok(),
            _ => std::env::var("ANTHROPIC_BASE_URL").ok(),
        }
    }

    // Determine the effective primary profile for API client construction.
    // After normalize_profiles, primary_profile is always Some if model is set.
    let primary_profile: config::ModelProfile = {
        let name = baoclaw_config.primary_profile.as_deref().unwrap_or("primary");
        baoclaw_config.model_profiles.get(name)
            .cloned()
            .unwrap_or_else(|| {
                // Fallback: construct from legacy fields
                config::ModelProfile {
                    model: baoclaw_config.model.clone(),
                    api_type: baoclaw_config.api_type.clone(),
                    api_key: None,
                    base_url: baoclaw_config.openai_base_url.clone(),
                    context_window: baoclaw_config.context_window,
                    auto_compact_threshold_ratio: baoclaw_config.auto_compact_threshold_ratio,
                    max_retries_per_model: baoclaw_config.max_retries_per_model,
                }
            })
    };

    // Get API key and config: use profile's api_type to pick env vars / credentials
    let api_client: Arc<UnifiedClient> = {
        let api_key = resolve_api_key(&primary_profile);
        let base_url = resolve_base_url(&primary_profile);
        match primary_profile.api_type.as_str() {
            "openai" => {
                eprintln!("Using OpenAI-compatible API (model: {}, base_url: {})",
                    primary_profile.model,
                    base_url.as_deref().unwrap_or("https://api.openai.com"));
                let config = ApiClientConfig {
                    api_key,
                    base_url,
                    max_retries: None,
                    api_path: None,
                };
                Arc::new(UnifiedClient::new_openai(config))
            }
            _ => {
                let api_path = std::env::var("ANTHROPIC_API_PATH").ok();
                eprintln!("Using Anthropic API (model: {}, base_url: {})",
                    primary_profile.model,
                    base_url.as_deref().unwrap_or("https://api.anthropic.com"));
                let config = ApiClientConfig {
                    api_key,
                    base_url,
                    max_retries: None,
                    api_path,
                };
                Arc::new(UnifiedClient::new_anthropic(config))
            }
        }
    };

    // Pre-warm the API connection pool in the background (TCP + TLS handshake
    // before the first real request, saving 100-300ms on first query).
    {
        let prewarm_client = Arc::clone(&api_client);
        tokio::spawn(async move {
            prewarm_client.prewarm().await;
        });
    }

    // Allow tools to access ~/.baoclaw/ in addition to project cwd
    let home_dir = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    let baoclaw_home = std::path::PathBuf::from(&home_dir).join(".baoclaw");
    let additional_dirs = vec![baoclaw_home];

    // Create evolution engine for self-improvement
    let evolution_engine = Arc::new(engine::evolution::EvolutionEngine::new(std::path::Path::new(&cwd_str)));

    // Build the core tool list (everything except AgentTool itself, which is added after)
    // BashTool is optionally sandboxed based on --sandbox CLI flag
    let bash_tool: BashTool = match &sandbox_config {
        Some(cfg) => BashTool::with_sandbox(Arc::clone(cfg)),
        None => BashTool::new(),
    };
    let core_tools: Vec<Arc<dyn tools::Tool>> = vec![
        Arc::new(bash_tool),
        Arc::new(FileReadTool::new(additional_dirs.clone())),
        Arc::new(FileWriteTool::new(additional_dirs.clone())),
        Arc::new(FileEditTool::new(additional_dirs.clone())),
        Arc::new(WebFetchTool::new()),
        Arc::new(WebSearchTool::new()),
        Arc::new(ImageGenTool::new()),
        Arc::new(ImageEditTool::new()),
        Arc::new(NotebookEditTool::new()),
        Arc::new(TodoWriteTool::new()),
        Arc::new(MemoryTool::new()),
        Arc::new(ProjectNoteTool::new()),
        Arc::new(tools::builtins::SkillTool::new(PathBuf::from(&cwd_str))),
        Arc::new(tools::builtins::EvolveTool::new(Arc::clone(&evolution_engine))),
    ];

    // AgentTool gets the full core tool set so sub-agents can write, edit, run bash, etc.
    let agent_tool = AgentTool::new_with_full_tools(Arc::clone(&api_client), core_tools.clone());

    let mut engine_tools: Vec<Arc<dyn tools::Tool>> = core_tools;
    engine_tools.push(Arc::new(agent_tool));

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

    // If daemon mode, fully detach from controlling terminal:
    //   1. setsid() — new session + new process group, no controlling terminal
    //   2. Ignore SIGHUP — extra safety against accidental kills
    // This prevents zombie accumulation: without setsid(), the daemon's ppid
    // stays as the launching terminal shell. When the terminal exits, orphaned
    // children get reparented to init slowly, and any subprocess zombies in the
    // daemon won't be reaped promptly.
    if is_daemon {
        #[cfg(unix)]
        unsafe {
            libc::setsid();
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

    let team_executor = Arc::new(engine::team::TeamManager::new(
        Arc::clone(&api_client),
        engine_tools.clone(),
        PathBuf::from(&cwd_str),
        baoclaw_config.model.clone(),
    ));

    // Create memory archive and cleanup scheduler for periodic memory maintenance
    let memory_archive = Arc::new(engine::memory::MemoryArchive::load());
    let memory_decay_config = engine::memory::DecayConfig::load();
    let memory_cleanup = Arc::new(engine::memory::MemoryCleanupScheduler::new(
        Arc::clone(&memory_store),
        Arc::clone(&memory_archive),
        memory_decay_config,
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
        session_id: session_id.clone(),
        should_exit: Arc::clone(&should_exit),
        session_registry: Arc::new(SessionRegistry::new()),
        skill_prompt: combined_append_prompt,
        memory_store,
        memory_archive,
        memory_cleanup,
        evolution_engine,
        cron_manager: Arc::new(engine::cron::CronManager::new()),
        project_registry: Arc::new(engine::projects::ProjectRegistry::new()),
        file_cache: Arc::new(tokio::sync::Mutex::new(engine::file_cache::FileCache::default_capacity())),
        tool_result_store: Some(Arc::new(engine::tool_result_store::ToolResultStore::for_session(&session_id))),
        hook_manager: Arc::new(engine::hooks::HookManager::new()),
        team_executor,
    };

    // ══════════════════════════════════════════════════════════
    // Start cron scheduler — runs periodic jobs in background.
    // Each job gets a fresh QueryEngine to execute its prompt,
    // and results are broadcast to all connected clients.
    // ══════════════════════════════════════════════════════════
    {
        let cron_manager = Arc::clone(&shared.cron_manager);
        let cron_tools = shared.engine_tools.clone();
        let cron_api_client = Arc::clone(&shared.api_client);
        let cron_baoclaw_config = shared.baoclaw_config.clone();
        let cron_thinking_config = shared.cli_thinking_config.clone();
        let cron_append_prompt = shared.skill_prompt.clone();
        let cron_session_id = shared.session_id.clone();
        let cron_file_cache = Arc::clone(&shared.file_cache);
        let cron_tool_result_store = shared.tool_result_store.as_ref().map(Arc::clone);
        let cron_hook_manager = Arc::clone(&shared.hook_manager);

        let run_fn: Arc<dyn Fn(String, Option<String>) -> tokio::task::JoinHandle<String> + Send + Sync> =
            Arc::new(move |prompt: String, cwd: Option<String>| {
                let tools = cron_tools.clone();
                let api_client = Arc::clone(&cron_api_client);
                let baoclaw_config = cron_baoclaw_config.clone();
                let thinking_config = cron_thinking_config.clone();
                let append_prompt = cron_append_prompt.clone();
                let _session_id = cron_session_id.clone();
                let file_cache = Arc::clone(&cron_file_cache);
                let tool_result_store = cron_tool_result_store.as_ref().map(Arc::clone);
                let hook_manager = Arc::clone(&cron_hook_manager);

                let job_session_id = format!("cron-{}", &uuid::Uuid::new_v4().to_string()[..8]);

                tokio::spawn(async move {
                    let cwd_path = cwd.map(PathBuf::from)
                        .unwrap_or_else(|| std::env::var("HOME")
                            .map(PathBuf::from)
                            .unwrap_or_else(|_| PathBuf::from("/tmp")));

                    let mut engine = QueryEngine::new(QueryEngineConfig {
                        cwd: cwd_path,
                        tools,
                        api_client,
                        model: baoclaw_config.model.clone(),
                        thinking_config,
                        max_turns: Some(10),
                        max_budget_usd: Some(0.5),
                        verbose: false,
                        custom_system_prompt: None,
                        append_system_prompt: append_prompt,
                        session_id: Some(job_session_id),
                        fallback_models: baoclaw_config.fallback_models.clone(),
                        max_retries_per_model: baoclaw_config.max_retries_per_model,
                        context_window: baoclaw_config.context_window,
                        auto_compact_threshold_ratio: baoclaw_config.auto_compact_threshold_ratio,
                        parent_turn_id: None,
                        agent_label: Some("cron".to_string()),
                        session_memory: None,
                        file_cache: Some(file_cache),
                        tool_result_store,
                        hook_manager: Some(hook_manager),
                    });

                    let mut rx = engine.submit_message(prompt).await;
                    let mut result = String::new();
                    while let Some(event) = rx.recv().await {
                        match event {
                            EngineEvent::AssistantChunk { content, .. } => result.push_str(&content),
                            EngineEvent::Result(qr) => {
                                if let Some(text) = qr.text {
                                    if !text.is_empty() && result.is_empty() {
                                        result = text;
                                    }
                                }
                                break;
                            }
                            EngineEvent::Error(_) => break,
                            _ => {}
                        }
                    }
                    if result.is_empty() {
                        result = "(no output)".to_string();
                    }
                    result
                })
            });

        tokio::spawn(async move {
            cron_manager.start_scheduler(run_fn).await;
        });
    }

    // ══════════════════════════════════════════════════════════
    // Start memory cleanup scheduler — runs periodic memory maintenance.
    // Applies decay to memories, archives low-importance ones,
    // and cleans up the archive when it exceeds max_entries.
    // ══════════════════════════════════════════════════════════
    {
        let memory_cleanup = Arc::clone(&shared.memory_cleanup);
        tokio::spawn(async move {
            memory_cleanup.start().await;
        });
    }

    // ══════════════════════════════════════════════════════════
    // Main accept loop — spawns a task per client connection
    // Multiple clients can be connected simultaneously, each
    // with its own independent QueryEngine / conversation history.
    // Only `shutdown` RPC terminates the daemon.
    // ══════════════════════════════════════════════════════════
    let should_exit_clone = Arc::clone(&should_exit);
    loop {
        if should_exit.load(Ordering::Relaxed) {
            eprintln!("should_exit detected — breaking accept loop");
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
            None => {
                eprintln!("should_exit watcher fired — breaking accept loop");
                break;
            }
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
