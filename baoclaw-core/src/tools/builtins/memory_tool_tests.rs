#[cfg(test)]
mod tests {
    use super::super::memory_tool::*;
    use crate::tools::trait_def::*;
    use tempfile::tempdir;
    use serde_json::json;

    struct NoopProgress;
    #[async_trait::async_trait]
    impl ProgressSender for NoopProgress {
        async fn send_progress(&self, _id: &str, _data: serde_json::Value) {}
    }

    fn make_ctx(path: &std::path::Path) -> ToolContext {
        let (_tx, rx) = tokio::sync::watch::channel(false);
        ToolContext {
            cwd: path.to_path_buf(),
            model: "test".into(),
            abort_signal: std::sync::Arc::new(rx),
            file_cache: None,
            tool_result_store: None,
            context_window: 100000,
            auto_compact_threshold_ratio: 0.8,
        }
    }

    #[tokio::test]
    async fn test_memory_tool_schema_and_name() {
        let tool = MemoryTool::new();
        assert_eq!(tool.name(), "MemoryTool");
        assert!(tool.aliases().contains(&"Memory"));
        assert!(!tool.prompt().is_empty());
        let schema = tool.input_schema();
        assert_eq!(schema.schema_type, "object");
    }

    #[tokio::test]
    async fn test_memory_tool_call_valid() {
        let tool = MemoryTool::new();
        let dir = tempdir().unwrap();
        let ctx = make_ctx(dir.path());
        let progress = NoopProgress;

        let input = json!({
            "content": "User prefers dark mode",
            "category": "preference"
        });

        let res = tool.call(input, &ctx, &progress).await;
        assert!(res.is_ok());
        let result = res.unwrap();
        assert!(!result.is_error);
        assert_eq!(result.data["category"], "preference");
    }

    #[tokio::test]
    async fn test_memory_tool_call_missing_fields() {
        let tool = MemoryTool::new();
        let dir = tempdir().unwrap();
        let ctx = make_ctx(dir.path());
        let progress = NoopProgress;

        let res = tool.call(json!({}), &ctx, &progress).await;
        assert!(res.is_err());
    }
}
