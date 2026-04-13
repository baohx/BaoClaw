use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;

use super::client::McpClient;
use super::client::McpToolDef;
use crate::tools::trait_def::{
    JsonSchema, ProgressSender, Tool, ToolContext, ToolError, ToolResult,
};

/// Wraps an MCP server tool as a BaoClaw Tool trait implementation.
pub struct McpToolWrapper {
    client: Arc<McpClient>,
    tool_def: McpToolDef,
    server_name: String,
}

impl McpToolWrapper {
    pub fn new(client: Arc<McpClient>, tool_def: McpToolDef, server_name: String) -> Self {
        Self {
            client,
            tool_def,
            server_name,
        }
    }
}

#[async_trait]
impl Tool for McpToolWrapper {
    fn name(&self) -> &str {
        &self.tool_def.name
    }

    fn input_schema(&self) -> JsonSchema {
        let schema = &self.tool_def.input_schema;
        JsonSchema {
            schema_type: schema["type"].as_str().unwrap_or("object").to_string(),
            properties: schema.get("properties").cloned(),
            required: schema.get("required").and_then(|v| {
                v.as_array()
                    .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            }),
            description: Some(self.tool_def.description.clone()),
        }
    }

    fn is_read_only(&self, _input: &Value) -> bool {
        false
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        false
    }

    async fn call(
        &self,
        input: Value,
        _context: &ToolContext,
        _progress: &dyn ProgressSender,
    ) -> Result<ToolResult, ToolError> {
        let result = self
            .client
            .call_tool(&self.tool_def.name, input)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("MCP tool error: {}", e)))?;

        let is_error = result
            .get("isError")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        Ok(ToolResult {
            data: result,
            is_error,
        })
    }

    fn prompt(&self) -> String {
        format!("[MCP:{}] {}", self.server_name, self.tool_def.description)
    }

    /// MCP tools (especially screenshot tools) can return large base64 images.
    /// Override the default 100K limit to 10MB to avoid truncating image data.
    fn max_result_size_chars(&self) -> usize {
        10_000_000
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::client::{McpClient, McpServerConfig, McpToolDef, McpTransportType};
    use std::collections::HashMap;

    fn test_client() -> Arc<McpClient> {
        Arc::new(McpClient::new(McpServerConfig {
            name: "test-server".to_string(),
            command: "echo".to_string(),
            args: vec![],
            env: HashMap::new(),
            transport: McpTransportType::Stdio,
        }))
    }

    fn test_tool_def() -> McpToolDef {
        McpToolDef {
            name: "read_file".to_string(),
            description: "Read a file from disk".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "File path" }
                },
                "required": ["path"]
            }),
        }
    }

    #[test]
    fn test_wrapper_name() {
        let wrapper = McpToolWrapper::new(
            test_client(),
            test_tool_def(),
            "test-server".to_string(),
        );
        assert_eq!(wrapper.name(), "read_file");
    }

    #[test]
    fn test_wrapper_prompt() {
        let wrapper = McpToolWrapper::new(
            test_client(),
            test_tool_def(),
            "test-server".to_string(),
        );
        assert_eq!(
            wrapper.prompt(),
            "[MCP:test-server] Read a file from disk"
        );
    }

    #[test]
    fn test_wrapper_input_schema_conversion() {
        let wrapper = McpToolWrapper::new(
            test_client(),
            test_tool_def(),
            "test-server".to_string(),
        );
        let schema = wrapper.input_schema();
        assert_eq!(schema.schema_type, "object");
        assert!(schema.properties.is_some());
        let props = schema.properties.unwrap();
        assert!(props.get("path").is_some());
        assert_eq!(schema.required, Some(vec!["path".to_string()]));
        assert_eq!(
            schema.description,
            Some("Read a file from disk".to_string())
        );
    }

    #[test]
    fn test_wrapper_schema_no_required() {
        let tool_def = McpToolDef {
            name: "list_files".to_string(),
            description: "List files".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "dir": { "type": "string" }
                }
            }),
        };
        let wrapper = McpToolWrapper::new(
            test_client(),
            tool_def,
            "srv".to_string(),
        );
        let schema = wrapper.input_schema();
        assert_eq!(schema.required, None);
    }

    #[test]
    fn test_wrapper_schema_empty() {
        let tool_def = McpToolDef {
            name: "ping".to_string(),
            description: "Ping server".to_string(),
            input_schema: serde_json::json!({}),
        };
        let wrapper = McpToolWrapper::new(
            test_client(),
            tool_def,
            "srv".to_string(),
        );
        let schema = wrapper.input_schema();
        assert_eq!(schema.schema_type, "object");
        assert_eq!(schema.properties, None);
        assert_eq!(schema.required, None);
    }

    #[test]
    fn test_wrapper_is_not_read_only() {
        let wrapper = McpToolWrapper::new(
            test_client(),
            test_tool_def(),
            "test-server".to_string(),
        );
        assert!(!wrapper.is_read_only(&Value::Null));
    }

    #[test]
    fn test_wrapper_is_not_concurrency_safe() {
        let wrapper = McpToolWrapper::new(
            test_client(),
            test_tool_def(),
            "test-server".to_string(),
        );
        assert!(!wrapper.is_concurrency_safe(&Value::Null));
    }
}
