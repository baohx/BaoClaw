use async_trait::async_trait;
use regex::Regex;
use serde::Serialize;
use serde_json::{json, Value};

use crate::tools::trait_def::*;

/// HTTP web fetch tool — fetches URL content and optionally converts HTML to plain text
pub struct WebFetchTool {
    http_client: reqwest::Client,
    max_size_bytes: usize,
}

impl WebFetchTool {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .user_agent("BaoClaw/1.0")
            .timeout(std::time::Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::limited(5))
            .build()
            .unwrap_or_default();
        Self {
            http_client: client,
            max_size_bytes: 1_048_576, // 1MB
        }
    }
}

#[derive(Debug, Serialize)]
struct FetchResult {
    content: String,
    status: u16,
    content_type: String,
    url: String,
}

#[async_trait]
impl Tool for WebFetchTool {
    fn name(&self) -> &str {
        "WebFetchTool"
    }

    fn aliases(&self) -> Vec<&str> {
        vec!["Fetch"]
    }

    fn input_schema(&self) -> JsonSchema {
        JsonSchema {
            schema_type: "object".to_string(),
            properties: Some(json!({
                "url": {
                    "type": "string",
                    "description": "The URL to fetch"
                },
                "raw": {
                    "type": "boolean",
                    "description": "Whether to return raw HTML (default false, returns plain text)"
                }
            })),
            required: Some(vec!["url".to_string()]),
            description: Some("Fetch web page content, supports HTML to plain text conversion".to_string()),
        }
    }

    fn is_read_only(&self, _input: &Value) -> bool {
        true
    }

    fn max_result_size_chars(&self) -> usize {
        30_000 // Limit fetched content to ~7.5k tokens
    }

    fn prompt(&self) -> String {
        "Fetch web page content from a URL. By default converts HTML to plain text. \
         Use raw=true to get the original HTML."
            .to_string()
    }

    async fn validate_input(&self, input: &Value, _context: &ToolContext) -> ValidationResult {
        match input.get("url").and_then(|v| v.as_str()) {
            Some(url) if !url.is_empty() => {
                if !url.starts_with("http://") && !url.starts_with("https://") {
                    ValidationResult::Invalid {
                        message: "URL must start with http:// or https://".to_string(),
                        code: None,
                    }
                } else {
                    ValidationResult::Ok
                }
            }
            _ => ValidationResult::Invalid {
                message: "Missing or empty 'url' field".to_string(),
                code: None,
            },
        }
    }

    async fn call(
        &self,
        input: Value,
        _context: &ToolContext,
        _progress: &dyn ProgressSender,
    ) -> Result<ToolResult, ToolError> {
        let url = input
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::ExecutionFailed("Missing 'url' field".to_string()))?;

        let raw = input
            .get("raw")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let result = fetch_and_convert(&self.http_client, url, raw, self.max_size_bytes).await?;

        Ok(ToolResult {
            data: json!({
                "content": result.content,
                "status": result.status,
                "content_type": result.content_type,
                "url": result.url,
            }),
            is_error: false,
        })
    }
}

async fn fetch_and_convert(
    client: &reqwest::Client,
    url: &str,
    raw: bool,
    max_size: usize,
) -> Result<FetchResult, ToolError> {
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err(ToolError::ExecutionFailed(
            "URL must start with http:// or https://".into(),
        ));
    }

    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| ToolError::ExecutionFailed(format!("HTTP request failed: {}", e)))?;

    let status = response.status().as_u16();
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown")
        .to_string();
    let final_url = response.url().to_string();

    let bytes = response
        .bytes()
        .await
        .map_err(|e| ToolError::ExecutionFailed(format!("Failed to read body: {}", e)))?;

    if bytes.len() > max_size {
        return Err(ToolError::ExecutionFailed(format!(
            "Response too large: {} bytes (max {})",
            bytes.len(),
            max_size
        )));
    }

    let body = String::from_utf8_lossy(&bytes).to_string();

    let content = if raw || !content_type.contains("html") {
        body
    } else {
        html_to_text(&body)
    };

    Ok(FetchResult {
        content,
        status,
        content_type,
        url: final_url,
    })
}

/// Simple HTML → plain text conversion
fn html_to_text(html: &str) -> String {
    let mut text = html.to_string();

    // Remove script and style tags and their content
    let script_re = Regex::new(r"(?is)<script[^>]*>.*?</script>").unwrap();
    let style_re = Regex::new(r"(?is)<style[^>]*>.*?</style>").unwrap();
    text = script_re.replace_all(&text, "").to_string();
    text = style_re.replace_all(&text, "").to_string();

    // Replace block-level tags with newlines
    let block_re = Regex::new(r"(?i)<(?:br|p|div|h[1-6]|li|tr)[^>]*>").unwrap();
    text = block_re.replace_all(&text, "\n").to_string();

    // Remove all remaining HTML tags
    let tag_re = Regex::new(r"<[^>]+>").unwrap();
    text = tag_re.replace_all(&text, "").to_string();

    // Decode common HTML entities
    text = text
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ");

    // Compress consecutive blank lines to max 2
    let multi_newline = Regex::new(r"\n{3,}").unwrap();
    text = multi_newline.replace_all(&text, "\n\n").to_string();

    text.trim().to_string()
}


#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

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
            abort_signal: Arc::new(rx),
        }
    }

    // --- Tool property tests ---

    #[test]
    fn test_web_fetch_tool_name() {
        let tool = WebFetchTool::new();
        assert_eq!(tool.name(), "WebFetchTool");
    }

    #[test]
    fn test_web_fetch_tool_aliases() {
        let tool = WebFetchTool::new();
        assert_eq!(tool.aliases(), vec!["Fetch"]);
    }

    #[test]
    fn test_web_fetch_tool_is_read_only() {
        let tool = WebFetchTool::new();
        assert!(tool.is_read_only(&json!({})));
    }

    #[test]
    fn test_web_fetch_tool_schema() {
        let tool = WebFetchTool::new();
        let schema = tool.input_schema();
        assert_eq!(schema.schema_type, "object");
        assert_eq!(schema.required, Some(vec!["url".to_string()]));
        let props = schema.properties.unwrap();
        assert!(props.get("url").is_some());
        assert!(props.get("raw").is_some());
    }

    // --- URL validation tests ---

    #[tokio::test]
    async fn test_validate_valid_https_url() {
        let tool = WebFetchTool::new();
        let ctx = make_context();
        let result = tool
            .validate_input(&json!({"url": "https://example.com"}), &ctx)
            .await;
        assert!(matches!(result, ValidationResult::Ok));
    }

    #[tokio::test]
    async fn test_validate_valid_http_url() {
        let tool = WebFetchTool::new();
        let ctx = make_context();
        let result = tool
            .validate_input(&json!({"url": "http://example.com"}), &ctx)
            .await;
        assert!(matches!(result, ValidationResult::Ok));
    }

    #[tokio::test]
    async fn test_validate_rejects_ftp_url() {
        let tool = WebFetchTool::new();
        let ctx = make_context();
        let result = tool
            .validate_input(&json!({"url": "ftp://example.com"}), &ctx)
            .await;
        assert!(matches!(result, ValidationResult::Invalid { .. }));
    }

    #[tokio::test]
    async fn test_validate_rejects_file_url() {
        let tool = WebFetchTool::new();
        let ctx = make_context();
        let result = tool
            .validate_input(&json!({"url": "file:///etc/passwd"}), &ctx)
            .await;
        assert!(matches!(result, ValidationResult::Invalid { .. }));
    }

    #[tokio::test]
    async fn test_validate_rejects_no_scheme() {
        let tool = WebFetchTool::new();
        let ctx = make_context();
        let result = tool
            .validate_input(&json!({"url": "example.com"}), &ctx)
            .await;
        assert!(matches!(result, ValidationResult::Invalid { .. }));
    }

    #[tokio::test]
    async fn test_validate_rejects_missing_url() {
        let tool = WebFetchTool::new();
        let ctx = make_context();
        let result = tool.validate_input(&json!({}), &ctx).await;
        assert!(matches!(result, ValidationResult::Invalid { .. }));
    }

    #[tokio::test]
    async fn test_validate_rejects_empty_url() {
        let tool = WebFetchTool::new();
        let ctx = make_context();
        let result = tool
            .validate_input(&json!({"url": ""}), &ctx)
            .await;
        assert!(matches!(result, ValidationResult::Invalid { .. }));
    }

    // --- html_to_text tests ---

    #[test]
    fn test_html_to_text_removes_script_tags() {
        let html = "<p>Hello</p><script>alert('xss')</script><p>World</p>";
        let text = html_to_text(html);
        assert!(!text.contains("alert"));
        assert!(!text.contains("<script>"));
        assert!(text.contains("Hello"));
        assert!(text.contains("World"));
    }

    #[test]
    fn test_html_to_text_removes_style_tags() {
        let html = "<style>body { color: red; }</style><p>Content</p>";
        let text = html_to_text(html);
        assert!(!text.contains("color"));
        assert!(!text.contains("<style>"));
        assert!(text.contains("Content"));
    }

    #[test]
    fn test_html_to_text_replaces_block_tags_with_newlines() {
        let html = "<div>Line1</div><div>Line2</div>";
        let text = html_to_text(html);
        assert!(text.contains("Line1"));
        assert!(text.contains("Line2"));
        // Block tags should produce newlines between content
        assert!(text.contains('\n'));
    }

    #[test]
    fn test_html_to_text_handles_br_tags() {
        let html = "Line1<br>Line2<br/>Line3";
        let text = html_to_text(html);
        assert!(text.contains("Line1"));
        assert!(text.contains("Line2"));
        assert!(text.contains("Line3"));
    }

    #[test]
    fn test_html_to_text_removes_all_tags() {
        let html = "<html><body><span class=\"x\">Hello</span> <a href=\"#\">World</a></body></html>";
        let text = html_to_text(html);
        assert!(!text.contains('<'));
        assert!(!text.contains('>'));
        assert!(text.contains("Hello"));
        assert!(text.contains("World"));
    }

    #[test]
    fn test_html_to_text_decodes_entities() {
        let html = "&amp; &lt; &gt; &quot; &#39; &nbsp;";
        let text = html_to_text(html);
        assert!(text.contains('&'));
        assert!(text.contains('<'));
        assert!(text.contains('>'));
        assert!(text.contains('"'));
        assert!(text.contains('\''));
    }

    #[test]
    fn test_html_to_text_compresses_blank_lines() {
        let html = "<p>A</p>\n\n\n\n\n<p>B</p>";
        let text = html_to_text(html);
        // Should not have more than 2 consecutive newlines
        assert!(!text.contains("\n\n\n"));
    }

    #[test]
    fn test_html_to_text_full_page() {
        let html = r#"<!DOCTYPE html>
<html>
<head><title>Test</title><style>body{margin:0}</style></head>
<body>
<h1>Title</h1>
<p>Paragraph with &amp; entity</p>
<script>var x = 1;</script>
<ul><li>Item 1</li><li>Item 2</li></ul>
</body>
</html>"#;
        let text = html_to_text(html);
        assert!(text.contains("Title"));
        assert!(text.contains("Paragraph with & entity"));
        assert!(text.contains("Item 1"));
        assert!(text.contains("Item 2"));
        assert!(!text.contains("var x"));
        assert!(!text.contains("margin"));
        assert!(!text.contains("<"));
    }

    #[test]
    fn test_html_to_text_empty_input() {
        let text = html_to_text("");
        assert_eq!(text, "");
    }

    #[test]
    fn test_html_to_text_plain_text_passthrough() {
        let text = html_to_text("Just plain text");
        assert_eq!(text, "Just plain text");
    }

    // --- fetch_and_convert URL validation ---

    #[tokio::test]
    async fn test_fetch_rejects_non_http_url() {
        let client = reqwest::Client::new();
        let result = fetch_and_convert(&client, "ftp://example.com", false, 1_048_576).await;
        assert!(result.is_err());
        let err = format!("{}", result.unwrap_err());
        assert!(err.contains("http://") || err.contains("https://"));
    }

    #[tokio::test]
    async fn test_fetch_rejects_file_url() {
        let client = reqwest::Client::new();
        let result = fetch_and_convert(&client, "file:///etc/passwd", false, 1_048_576).await;
        assert!(result.is_err());
    }
}
