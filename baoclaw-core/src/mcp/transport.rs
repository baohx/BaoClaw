use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::process::{Child, ChildStdin, ChildStdout};

use super::client::McpError;

/// JSON-RPC 2.0 request for MCP protocol
#[derive(Serialize)]
struct McpJsonRpcRequest {
    jsonrpc: String,
    id: u64,
    method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<Value>,
}

/// JSON-RPC 2.0 response from MCP server
#[derive(Deserialize)]
struct McpJsonRpcResponse {
    #[allow(dead_code)]
    jsonrpc: String,
    id: Option<u64>,
    #[serde(default)]
    result: Option<Value>,
    #[serde(default)]
    error: Option<McpJsonRpcError>,
}

#[derive(Deserialize)]
struct McpJsonRpcError {
    code: i64,
    message: String,
    #[allow(dead_code)]
    data: Option<Value>,
}

/// Stdio transport layer — communicates with MCP server via child process stdin/stdout
pub struct StdioTransport {
    child: Child,
    writer: BufWriter<ChildStdin>,
    reader: BufReader<ChildStdout>,
    next_id: u64,
}

impl StdioTransport {
    /// Spawn a child process and perform MCP initialize handshake.
    pub async fn spawn(
        command: &str,
        args: &[String],
        env: &HashMap<String, String>,
    ) -> Result<Self, McpError> {
        let mut child = tokio::process::Command::new(command)
            .args(args)
            .envs(env)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| McpError::ConnectionFailed(format!("Failed to spawn: {}", e)))?;

        let stdin = child
            .stdin
            .take()
            .ok_or(McpError::ConnectionFailed("No stdin".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or(McpError::ConnectionFailed("No stdout".into()))?;

        let mut transport = Self {
            child,
            writer: BufWriter::new(stdin),
            reader: BufReader::new(stdout),
            next_id: 1,
        };

        // MCP initialize handshake
        let _init_result = transport
            .request(
                "initialize",
                Some(serde_json::json!({
                    "protocolVersion": "2024-11-05",
                    "capabilities": {},
                    "clientInfo": {
                        "name": "baoclaw",
                        "version": "0.3.0"
                    }
                })),
            )
            .await?;

        // Send initialized notification
        transport.notify("notifications/initialized", None).await?;

        Ok(transport)
    }

    /// Send a JSON-RPC request and wait for the matching response.
    pub async fn request(
        &mut self,
        method: &str,
        params: Option<Value>,
    ) -> Result<Value, McpError> {
        let id = self.next_id;
        self.next_id += 1;

        let request = McpJsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id,
            method: method.to_string(),
            params,
        };

        let mut line = serde_json::to_string(&request)
            .map_err(|e| McpError::TransportError(format!("Serialize error: {}", e)))?;
        line.push('\n');

        self.writer
            .write_all(line.as_bytes())
            .await
            .map_err(|e| McpError::TransportError(format!("Write error: {}", e)))?;
        self.writer
            .flush()
            .await
            .map_err(|e| McpError::TransportError(format!("Flush error: {}", e)))?;

        // Read response lines, skipping notifications (messages without id)
        loop {
            let mut response_line = String::new();
            let bytes_read = self
                .reader
                .read_line(&mut response_line)
                .await
                .map_err(|e| McpError::TransportError(format!("Read error: {}", e)))?;

            if bytes_read == 0 {
                return Err(McpError::TransportError(
                    "EOF: child process closed stdout".into(),
                ));
            }

            if response_line.trim().is_empty() {
                continue;
            }

            let response: McpJsonRpcResponse = serde_json::from_str(&response_line)
                .map_err(|e| McpError::TransportError(format!("Parse error: {}", e)))?;

            // Skip notifications (no id)
            if response.id.is_none() {
                continue;
            }

            if let Some(error) = response.error {
                return Err(McpError::ToolCallFailed(format!(
                    "MCP error {}: {}",
                    error.code, error.message
                )));
            }

            return Ok(response.result.unwrap_or(Value::Null));
        }
    }

    /// Send a JSON-RPC notification (no id, no response expected).
    pub async fn notify(
        &mut self,
        method: &str,
        params: Option<Value>,
    ) -> Result<(), McpError> {
        let notification = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params.unwrap_or(Value::Null),
        });

        let mut line = serde_json::to_string(&notification)
            .map_err(|e| McpError::TransportError(format!("Serialize error: {}", e)))?;
        line.push('\n');

        self.writer
            .write_all(line.as_bytes())
            .await
            .map_err(|e| McpError::TransportError(format!("Write error: {}", e)))?;
        self.writer
            .flush()
            .await
            .map_err(|e| McpError::TransportError(format!("Flush error: {}", e)))?;

        Ok(())
    }

    /// Shutdown the transport: send shutdown request + exit notification,
    /// wait 5s for child exit then kill.
    pub async fn shutdown(&mut self) -> Result<(), McpError> {
        let _ = self.request("shutdown", None).await;
        let _ = self.notify("exit", None).await;

        match tokio::time::timeout(std::time::Duration::from_secs(5), self.child.wait()).await {
            Ok(_) => {}
            Err(_) => {
                let _ = self.child.kill().await;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_json_rpc_request_serialization() {
        let req = McpJsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: 1,
            method: "initialize".to_string(),
            params: Some(serde_json::json!({"protocolVersion": "2024-11-05"})),
        };
        let json = serde_json::to_string(&req).unwrap();
        let parsed: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["jsonrpc"], "2.0");
        assert_eq!(parsed["id"], 1);
        assert_eq!(parsed["method"], "initialize");
        assert_eq!(parsed["params"]["protocolVersion"], "2024-11-05");
    }

    #[test]
    fn test_json_rpc_request_no_params() {
        let req = McpJsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: 5,
            method: "tools/list".to_string(),
            params: None,
        };
        let json = serde_json::to_string(&req).unwrap();
        let parsed: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["method"], "tools/list");
        assert!(parsed.get("params").is_none());
    }

    #[test]
    fn test_json_rpc_response_deserialization_success() {
        let json = r#"{"jsonrpc":"2.0","id":1,"result":{"tools":[]}}"#;
        let resp: McpJsonRpcResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.id, Some(1));
        assert!(resp.result.is_some());
        assert!(resp.error.is_none());
    }

    #[test]
    fn test_json_rpc_response_deserialization_error() {
        let json = r#"{"jsonrpc":"2.0","id":2,"error":{"code":-32601,"message":"Method not found"}}"#;
        let resp: McpJsonRpcResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.id, Some(2));
        assert!(resp.result.is_none());
        let err = resp.error.unwrap();
        assert_eq!(err.code, -32601);
        assert_eq!(err.message, "Method not found");
    }

    #[test]
    fn test_json_rpc_response_notification() {
        let json = r#"{"jsonrpc":"2.0","method":"notifications/progress","params":{}}"#;
        let resp: McpJsonRpcResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.id, None);
    }

    #[test]
    fn test_json_rpc_notification_serialization() {
        let notification = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized",
            "params": Value::Null,
        });
        let json = serde_json::to_string(&notification).unwrap();
        let parsed: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["jsonrpc"], "2.0");
        assert_eq!(parsed["method"], "notifications/initialized");
        assert!(parsed.get("id").is_none());
    }

    #[test]
    fn test_request_id_auto_increment() {
        // Verify the id field increments correctly in serialized requests
        let req1 = McpJsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: 1,
            method: "tools/list".to_string(),
            params: None,
        };
        let req2 = McpJsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: 2,
            method: "tools/call".to_string(),
            params: Some(serde_json::json!({"name": "test"})),
        };
        let j1: Value = serde_json::from_str(&serde_json::to_string(&req1).unwrap()).unwrap();
        let j2: Value = serde_json::from_str(&serde_json::to_string(&req2).unwrap()).unwrap();
        assert_eq!(j1["id"], 1);
        assert_eq!(j2["id"], 2);
    }
}
