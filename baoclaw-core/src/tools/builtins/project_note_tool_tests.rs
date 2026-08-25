#[cfg(test)]
mod tests {
    use super::super::project_note_tool::*;
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
    async fn test_project_note_tool_basic() {
        let tool = ProjectNoteTool::new();
        assert_eq!(tool.name(), "ProjectNoteTool");
        assert!(tool.aliases().contains(&"ProjectNote"));
        assert!(!tool.prompt().is_empty());

        let dir = tempdir().unwrap();
        let ctx = make_ctx(dir.path());
        let progress = NoopProgress;

        let input = json!({ "note": "Always run cargo clippy before commit" });
        let res = tool.call(input, &ctx, &progress).await;
        assert!(res.is_ok());

        let baoclaw_md = dir.path().join("BAOCLAW.md");
        assert!(baoclaw_md.exists());
        let content = std::fs::read_to_string(baoclaw_md).unwrap();
        assert!(content.contains("Always run cargo clippy before commit"));
    }
}
