use async_trait::async_trait;
use serde::Serialize;
use serde_json::{json, Value};

use crate::tools::trait_def::*;

/// Web search tool — searches the web via Brave Search API
pub struct WebSearchTool {
    http_client: reqwest::Client,
    api_key: Option<String>,
}

impl WebSearchTool {
    pub fn new() -> Self {
        Self {
            http_client: reqwest::Client::new(),
            api_key: std::env::var("BRAVE_SEARCH_API_KEY").ok(),
        }
    }
}

#[derive(Debug, Serialize)]
struct SearchResult {
    title: String,
    url: String,
    snippet: String,
}

#[async_trait]
impl Tool for WebSearchTool {
    fn name(&self) -> &str {
        "WebSearchTool"
    }

    fn aliases(&self) -> Vec<&str> {
        vec!["Search"]
    }

    fn input_schema(&self) -> JsonSchema {
        JsonSchema {
            schema_type: "object".to_string(),
            properties: Some(json!({
                "query": {
                    "type": "string",
                    "description": "Search query"
                },
                "num_results": {
                    "type": "integer",
                    "description": "Number of results to return (default 5, max 20)"
                }
            })),
            required: Some(vec!["query".to_string()]),
            description: Some("Search the web via Brave Search API".to_string()),
        }
    }

    fn is_read_only(&self, _input: &Value) -> bool {
        true
    }

    fn prompt(&self) -> String {
        "Search the web for information using Brave Search API.".to_string()
    }

    async fn call(
        &self,
        input: Value,
        _context: &ToolContext,
        _progress: &dyn ProgressSender,
    ) -> Result<ToolResult, ToolError> {
        let api_key = match &self.api_key {
            Some(key) if !key.is_empty() => key.clone(),
            _ => {
                return Ok(ToolResult {
                    data: json!({ "error": "BRAVE_SEARCH_API_KEY not set" }),
                    is_error: true,
                });
            }
        };

        let query = input
            .get("query")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::ExecutionFailed("Missing 'query' field".to_string()))?;

        let num_results = input
            .get("num_results")
            .and_then(|v| v.as_u64())
            .unwrap_or(5)
            .min(20) as usize;

        let results = search(&self.http_client, &api_key, query, num_results).await?;
        let count = results.len();

        Ok(ToolResult {
            data: json!({
                "results": results,
                "count": count,
            }),
            is_error: false,
        })
    }
}

async fn search(
    client: &reqwest::Client,
    api_key: &str,
    query: &str,
    num_results: usize,
) -> Result<Vec<SearchResult>, ToolError> {
    let url = format!(
        "https://api.search.brave.com/res/v1/web/search?q={}&count={}",
        urlencoding::encode(query),
        num_results.min(20)
    );

    let max_retries = 3;
    let mut last_status = reqwest::StatusCode::OK;

    for attempt in 0..=max_retries {
        if attempt > 0 {
            // Exponential backoff: 2s, 4s, 8s
            let delay = std::time::Duration::from_secs(2u64.pow(attempt as u32));
            eprintln!("WebSearch: 429 rate limited, retrying in {}s (attempt {}/{})", delay.as_secs(), attempt, max_retries);
            tokio::time::sleep(delay).await;
        }

        let response = client
            .get(&url)
            .header("X-Subscription-Token", api_key)
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("Search API error: {}", e)))?;

        last_status = response.status();

        if last_status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            continue; // retry
        }

        if !last_status.is_success() {
            return Err(ToolError::ExecutionFailed(format!(
                "Search API returned status {}",
                last_status
            )));
        }

        let body: Value = response
            .json()
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to parse response: {}", e)))?;

        let results = body["web"]["results"]
            .as_array()
            .unwrap_or(&Vec::new())
            .iter()
            .map(|r| SearchResult {
                title: r["title"].as_str().unwrap_or("").to_string(),
                url: r["url"].as_str().unwrap_or("").to_string(),
                snippet: r["description"].as_str().unwrap_or("").to_string(),
            })
            .collect();

        return Ok(results);
    }

    Err(ToolError::ExecutionFailed(format!(
        "Search API returned status {} after {} retries",
        last_status, max_retries
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_web_search_tool_name() {
        let tool = WebSearchTool::new();
        assert_eq!(tool.name(), "WebSearchTool");
    }

    #[test]
    fn test_web_search_tool_aliases() {
        let tool = WebSearchTool::new();
        assert_eq!(tool.aliases(), vec!["Search"]);
    }

    #[test]
    fn test_web_search_tool_is_read_only() {
        let tool = WebSearchTool::new();
        assert!(tool.is_read_only(&json!({})));
    }

    #[test]
    fn test_web_search_tool_schema() {
        let tool = WebSearchTool::new();
        let schema = tool.input_schema();
        assert_eq!(schema.schema_type, "object");
        assert_eq!(schema.required, Some(vec!["query".to_string()]));
        let props = schema.properties.unwrap();
        assert!(props.get("query").is_some());
        assert!(props.get("num_results").is_some());
    }

    #[tokio::test]
    async fn test_missing_api_key_returns_error() {
        let tool = WebSearchTool {
            http_client: reqwest::Client::new(),
            api_key: None,
        };
        let ctx = make_context();
        let progress = NoopProgress;
        let result = tool
            .call(json!({"query": "test"}), &ctx, &progress)
            .await
            .unwrap();
        assert!(result.is_error);
        assert!(result.data["error"]
            .as_str()
            .unwrap()
            .contains("BRAVE_SEARCH_API_KEY not set"));
    }

    #[tokio::test]
    async fn test_empty_api_key_returns_error() {
        let tool = WebSearchTool {
            http_client: reqwest::Client::new(),
            api_key: Some("".to_string()),
        };
        let ctx = make_context();
        let progress = NoopProgress;
        let result = tool
            .call(json!({"query": "test"}), &ctx, &progress)
            .await
            .unwrap();
        assert!(result.is_error);
    }

    struct NoopProgress;
    #[async_trait]
    impl ProgressSender for NoopProgress {
        async fn send_progress(&self, _id: &str, _data: Value) {}
    }

    fn make_context() -> ToolContext {
        let (_tx, rx) = tokio::sync::watch::channel(false);
        ToolContext {
            cwd: std::path::PathBuf::from("/tmp"),
            model: "test".to_string(),
            abort_signal: std::sync::Arc::new(rx),
        }
    }
}
