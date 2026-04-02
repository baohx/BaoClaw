use std::sync::Arc;

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

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Create socket path in temp directory using process ID for uniqueness
    let socket_path = std::env::temp_dir()
        .join(format!("claude-core-{}.sock", std::process::id()));

    // 2. Bind IPC server to the Unix Domain Socket
    let server = IpcServer::bind(&socket_path).await?;

    // 3. Output socket path for the TypeScript process to read
    println!("SOCKET:{}", socket_path.display());

    // Flush stdout to ensure TS process reads it immediately
    use std::io::Write;
    std::io::stdout().flush()?;

    // 4. Wait for a client connection
    let mut conn = server.accept().await?;

    // 5. Wait for the initialize request
    let init_msg = conn.recv_message().await?;
    let (init_id, init_config) = match init_msg {
        JsonRpcMessage::Request(req) => {
            let id = req.id.clone();
            match parse_client_method(&req) {
                Ok(method) => match method {
                    ClientMethod::Initialize { cwd, model, settings: _ } => {
                        (id, (cwd, model))
                    }
                    _ => {
                        conn.send_error(
                            Some(req.id),
                            -32600,
                            "Expected 'initialize' as first request".into(),
                        ).await?;
                        return Ok(());
                    }
                },
                Err(e) => {
                    conn.send_error(
                        Some(req.id),
                        -32600,
                        format!("Invalid initialize request: {}", e),
                    ).await?;
                    return Ok(());
                }
            }
        }
        _ => {
            return Err("Expected JSON-RPC request for initialization".into());
        }
    };

    let (cwd, model_opt) = init_config;
    let model = model_opt.unwrap_or_else(|| "claude-sonnet-4-20250514".to_string());

    // Get API key from environment
    let api_key = std::env::var("ANTHROPIC_API_KEY").unwrap_or_default();

    let api_client = Arc::new(AnthropicClient::new(ApiClientConfig {
        api_key,
        base_url: std::env::var("ANTHROPIC_BASE_URL").ok(),
        max_retries: None,
    }));

    // Assemble tool pool with built-in tools
    let engine_tools: Vec<Arc<dyn tools::Tool>> = vec![
        Arc::new(BashTool::new()),
        Arc::new(FileReadTool::new(vec![])),
        Arc::new(FileWriteTool::new(vec![])),
        Arc::new(FileEditTool::new(vec![])),
    ];

    // Create the QueryEngine with built-in tools
    let mut engine = QueryEngine::new(QueryEngineConfig {
        cwd: cwd.clone(),
        tools: engine_tools.clone(),
        api_client,
        model: model.clone(),
        thinking_config: ThinkingConfig::Disabled,
        max_turns: None,
        max_budget_usd: None,
        verbose: false,
        custom_system_prompt: None,
        append_system_prompt: None,
    });

    // Create state manager
    let state_manager = StateManager::new(CoreState {
        session_id: uuid::Uuid::new_v4().to_string(),
        model: model.clone(),
        verbose: false,
        tasks: std::collections::HashMap::new(),
        usage: EMPTY_USAGE,
        total_cost_usd: 0.0,
    });

    // Send initialize response with capabilities
    conn.send_response(
        init_id,
        serde_json::json!({
            "capabilities": {
                "tools": true,
                "streaming": true,
                "permissions": true,
            },
            "session_id": state_manager.get().session_id,
        }),
    ).await?;

    // Spawn a task to forward state patches over IPC
    // We use a separate channel to forward patches from the broadcast receiver
    // to the main loop, since the IPC connection isn't Send-safe for direct use in spawned tasks.
    let (patch_tx, mut patch_rx) = tokio::sync::mpsc::channel::<Vec<state::manager::StatePatch>>(256);
    {
        let mut state_rx = state_manager.subscribe();
        tokio::spawn(async move {
            loop {
                match state_rx.recv().await {
                    Ok(patches) => {
                        if patch_tx.send(patches).await.is_err() {
                            // Main loop dropped the receiver, exit
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        eprintln!("State patch subscriber lagged by {} messages", n);
                        // Continue receiving
                    }
                }
            }
        });
    }

    // 6. Main RPC loop
    loop {
        // Use select to handle both IPC messages and state patches
        tokio::select! {
            // Forward state patches to the client
            Some(patches) = patch_rx.recv() => {
                if let Err(e) = send_state_patches(&mut conn, &patches).await {
                    eprintln!("Failed to send state patches: {}", e);
                }
            }
            // Handle incoming IPC messages
            msg_result = conn.recv_message() => {
                let msg = match msg_result {
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
                            Ok(method) => match method {
                                ClientMethod::SubmitMessage { prompt, .. } => {
                                    let prompt_str = match prompt.as_str() {
                                        Some(s) => s.to_string(),
                                        None => serde_json::to_string(&prompt).unwrap_or_default(),
                                    };

                                    let mut rx = engine.submit_message(prompt_str).await;

                                    // Stream events to client
                                    while let Some(event) = rx.recv().await {
                                        if let Err(e) = send_engine_event(&mut conn, &event).await {
                                            eprintln!("Failed to send event: {}", e);
                                            break;
                                        }
                                        // If this is a terminal event, stop streaming
                                        if matches!(event, EngineEvent::Result(_) | EngineEvent::Error(_)) {
                                            break;
                                        }
                                    }

                                    conn.send_response(id, serde_json::json!({"status": "complete"})).await?;
                                }
                                ClientMethod::Abort => {
                                    engine.abort();
                                    conn.send_response(id, serde_json::json!("ok")).await?;
                                }
                                ClientMethod::UpdateSettings { settings: _ } => {
                                    // TODO: apply settings updates
                                    conn.send_response(id, serde_json::json!("ok")).await?;
                                }
                                ClientMethod::PermissionResponse { .. } => {
                                    // TODO: route permission response to waiting tool
                                    conn.send_response(id, serde_json::json!("ok")).await?;
                                }
                                ClientMethod::Shutdown => {
                                    conn.send_response(id, serde_json::json!("ok")).await?;
                                    break;
                                }
                                ClientMethod::Initialize { .. } => {
                                    conn.send_error(
                                        Some(id),
                                        -32600,
                                        "Already initialized".into(),
                                    ).await?;
                                }
                                ClientMethod::ListTools => {
                                    let tool_list: Vec<serde_json::Value> = engine_tools.iter().map(|t| {
                                        serde_json::json!({
                                            "name": t.name(),
                                            "description": t.prompt(),
                                            "type": "builtin",
                                        })
                                    }).collect();
                                    conn.send_response(id, serde_json::json!({
                                        "tools": tool_list,
                                        "count": tool_list.len(),
                                    })).await?;
                                }
                                ClientMethod::ListMcpServers => {
                                    let servers = discovery::mcp_config::discover_mcp_servers(&cwd).await;
                                    conn.send_response(id, serde_json::json!({
                                        "servers": servers,
                                        "count": servers.len(),
                                    })).await?;
                                }
                                ClientMethod::ListSkills => {
                                    let skills = discovery::skills::discover_skills(&cwd).await;
                                    conn.send_response(id, serde_json::json!({
                                        "skills": skills,
                                        "count": skills.len(),
                                    })).await?;
                                }
                                ClientMethod::ListPlugins => {
                                    let plugins = discovery::plugins::discover_plugins(&cwd).await;
                                    conn.send_response(id, serde_json::json!({
                                        "plugins": plugins,
                                        "count": plugins.len(),
                                    })).await?;
                                }
                            },
                            Err(e) => {
                                conn.send_error(Some(id), -32601, format!("{}", e)).await?;
                            }
                        }
                    }
                    _ => {
                        // Ignore non-request messages (notifications, responses)
                    }
                }
            }
        }
    }

    // 7. Cleanup - IpcServer's Drop impl removes the socket file
    drop(server);
    eprintln!("claude-core shutdown complete");

    Ok(())
}
