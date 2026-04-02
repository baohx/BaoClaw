use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use super::transport::StdioTransport;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct McpServerConfig {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
    pub transport: McpTransportType,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum McpTransportType {
    Stdio,
    Sse { url: String },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct McpToolDef {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct McpResource {
    pub uri: String,
    pub name: String,
    pub description: Option<String>,
    pub mime_type: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum McpConnectionStatus {
    Connected,
    Disconnected,
    Reconnecting,
}

#[derive(Debug, thiserror::Error)]
pub enum McpError {
    #[error("Not connected")]
    NotConnected,
    #[error("Connection failed: {0}")]
    ConnectionFailed(String),
    #[error("Tool not found: {0}")]
    ToolNotFound(String),
    #[error("Tool call failed: {0}")]
    ToolCallFailed(String),
    #[error("Transport error: {0}")]
    TransportError(String),
}

pub struct McpClient {
    config: McpServerConfig,
    status: Arc<RwLock<McpConnectionStatus>>,
    tools: Arc<RwLock<Vec<McpToolDef>>>,
    transport: Option<Arc<RwLock<StdioTransport>>>,
}

impl McpClient {
    pub fn new(config: McpServerConfig) -> Self {
        Self {
            config,
            status: Arc::new(RwLock::new(McpConnectionStatus::Disconnected)),
            tools: Arc::new(RwLock::new(Vec::new())),
            transport: None,
        }
    }

    /// Connect to the MCP server (stub — sets status without transport).
    pub async fn connect(&self) -> Result<(), McpError> {
        let mut status = self.status.write().await;
        *status = McpConnectionStatus::Connected;
        Ok(())
    }

    /// Connect via stdio transport: spawn child process, handshake, refresh tools.
    pub async fn connect_stdio(&mut self) -> Result<(), McpError> {
        let transport = StdioTransport::spawn(
            &self.config.command,
            &self.config.args,
            &self.config.env,
        )
        .await?;

        self.transport = Some(Arc::new(RwLock::new(transport)));
        *self.status.write().await = McpConnectionStatus::Connected;

        self.refresh_tools().await?;
        Ok(())
    }

    /// Refresh the cached tool list from the MCP server.
    pub async fn refresh_tools(&self) -> Result<(), McpError> {
        let transport = self.transport.as_ref().ok_or(McpError::NotConnected)?;
        let mut t = transport.write().await;

        let result = t.request("tools/list", None).await?;
        let tool_defs: Vec<McpToolDef> =
            serde_json::from_value(result["tools"].clone()).unwrap_or_default();

        *self.tools.write().await = tool_defs;
        Ok(())
    }

    /// Get the list of tools provided by this server.
    pub async fn list_tools(&self) -> Result<Vec<McpToolDef>, McpError> {
        let status = self.status.read().await;
        if *status != McpConnectionStatus::Connected {
            return Err(McpError::NotConnected);
        }
        let tools = self.tools.read().await;
        Ok(tools.clone())
    }

    /// Call a tool on the MCP server.
    /// Uses transport when available, otherwise falls back to stub.
    pub async fn call_tool(&self, name: &str, args: Value) -> Result<Value, McpError> {
        let status = self.status.read().await;
        if *status != McpConnectionStatus::Connected {
            return Err(McpError::NotConnected);
        }

        let tools = self.tools.read().await;
        let tool_exists = tools.iter().any(|t| t.name == name);
        if !tool_exists {
            return Err(McpError::ToolNotFound(name.to_string()));
        }
        drop(tools);

        // Use transport if available
        if let Some(ref transport) = self.transport {
            let mut t = transport.write().await;
            let result = t
                .request(
                    "tools/call",
                    Some(serde_json::json!({
                        "name": name,
                        "arguments": args,
                    })),
                )
                .await?;
            return Ok(result);
        }

        // Stub fallback for testing
        Ok(Value::Object(serde_json::Map::new()))
    }

    /// List resources provided by the MCP server.
    pub async fn list_resources(&self) -> Result<Vec<McpResource>, McpError> {
        let transport = self.transport.as_ref().ok_or(McpError::NotConnected)?;
        let mut t = transport.write().await;

        let result = t.request("resources/list", None).await?;
        let resources: Vec<McpResource> =
            serde_json::from_value(result["resources"].clone()).unwrap_or_default();

        Ok(resources)
    }

    /// Read a specific resource by URI.
    pub async fn read_resource(&self, uri: &str) -> Result<Value, McpError> {
        let transport = self.transport.as_ref().ok_or(McpError::NotConnected)?;
        let mut t = transport.write().await;

        let result = t
            .request(
                "resources/read",
                Some(serde_json::json!({
                    "uri": uri,
                })),
            )
            .await?;

        Ok(result)
    }

    /// Get the current connection status.
    pub async fn status(&self) -> McpConnectionStatus {
        self.status.read().await.clone()
    }

    /// Disconnect from the MCP server.
    pub async fn disconnect(&self) -> Result<(), McpError> {
        let mut status = self.status.write().await;
        *status = McpConnectionStatus::Disconnected;
        Ok(())
    }

    /// Get the server config.
    pub fn config(&self) -> &McpServerConfig {
        &self.config
    }

    /// Add tools to the cached list (used for testing and initialization).
    pub async fn set_tools(&self, new_tools: Vec<McpToolDef>) {
        let mut tools = self.tools.write().await;
        *tools = new_tools;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> McpServerConfig {
        McpServerConfig {
            name: "test-server".to_string(),
            command: "test-cmd".to_string(),
            args: vec![],
            env: HashMap::new(),
            transport: McpTransportType::Stdio,
        }
    }

    fn sample_tools() -> Vec<McpToolDef> {
        vec![
            McpToolDef {
                name: "read_file".to_string(),
                description: "Read a file".to_string(),
                input_schema: serde_json::json!({"type": "object"}),
            },
            McpToolDef {
                name: "write_file".to_string(),
                description: "Write a file".to_string(),
                input_schema: serde_json::json!({"type": "object"}),
            },
        ]
    }

    #[tokio::test]
    async fn test_new_client_is_disconnected() {
        let client = McpClient::new(test_config());
        assert_eq!(client.status().await, McpConnectionStatus::Disconnected);
    }

    #[tokio::test]
    async fn test_connect_sets_status() {
        let client = McpClient::new(test_config());
        client.connect().await.unwrap();
        assert_eq!(client.status().await, McpConnectionStatus::Connected);
    }

    #[tokio::test]
    async fn test_disconnect_sets_status() {
        let client = McpClient::new(test_config());
        client.connect().await.unwrap();
        client.disconnect().await.unwrap();
        assert_eq!(client.status().await, McpConnectionStatus::Disconnected);
    }

    #[tokio::test]
    async fn test_list_tools_requires_connection() {
        let client = McpClient::new(test_config());
        let result = client.list_tools().await;
        assert!(matches!(result, Err(McpError::NotConnected)));
    }

    #[tokio::test]
    async fn test_list_tools_returns_cached() {
        let client = McpClient::new(test_config());
        client.connect().await.unwrap();
        client.set_tools(sample_tools()).await;

        let tools = client.list_tools().await.unwrap();
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0].name, "read_file");
        assert_eq!(tools[1].name, "write_file");
    }

    #[tokio::test]
    async fn test_list_tools_empty_when_none_set() {
        let client = McpClient::new(test_config());
        client.connect().await.unwrap();
        let tools = client.list_tools().await.unwrap();
        assert!(tools.is_empty());
    }

    #[tokio::test]
    async fn test_call_tool_requires_connection() {
        let client = McpClient::new(test_config());
        let result = client.call_tool("read_file", Value::Null).await;
        assert!(matches!(result, Err(McpError::NotConnected)));
    }

    #[tokio::test]
    async fn test_call_tool_not_found() {
        let client = McpClient::new(test_config());
        client.connect().await.unwrap();
        client.set_tools(sample_tools()).await;

        let result = client.call_tool("nonexistent", Value::Null).await;
        assert!(matches!(result, Err(McpError::ToolNotFound(_))));
    }

    #[tokio::test]
    async fn test_call_tool_success() {
        let client = McpClient::new(test_config());
        client.connect().await.unwrap();
        client.set_tools(sample_tools()).await;

        let result = client
            .call_tool("read_file", serde_json::json!({"path": "/tmp/test"}))
            .await
            .unwrap();
        assert!(result.is_object());
    }

    #[tokio::test]
    async fn test_call_tool_after_disconnect() {
        let client = McpClient::new(test_config());
        client.connect().await.unwrap();
        client.set_tools(sample_tools()).await;

        // Works while connected
        client.call_tool("read_file", Value::Null).await.unwrap();

        // Fails after disconnect
        client.disconnect().await.unwrap();
        let result = client.call_tool("read_file", Value::Null).await;
        assert!(matches!(result, Err(McpError::NotConnected)));
    }

    #[tokio::test]
    async fn test_reconnect_restores_functionality() {
        let client = McpClient::new(test_config());
        client.connect().await.unwrap();
        client.set_tools(sample_tools()).await;
        client.disconnect().await.unwrap();

        // Reconnect
        client.connect().await.unwrap();
        // Tools are still cached
        let tools = client.list_tools().await.unwrap();
        assert_eq!(tools.len(), 2);
        client.call_tool("read_file", Value::Null).await.unwrap();
    }

    #[tokio::test]
    async fn test_sse_transport_config() {
        let config = McpServerConfig {
            name: "sse-server".to_string(),
            command: "".to_string(),
            args: vec![],
            env: HashMap::new(),
            transport: McpTransportType::Sse {
                url: "https://example.com/sse".to_string(),
            },
        };
        let client = McpClient::new(config);
        assert_eq!(client.config().name, "sse-server");
        match &client.config().transport {
            McpTransportType::Sse { url } => assert_eq!(url, "https://example.com/sse"),
            _ => panic!("Expected SSE transport"),
        }
    }
}
