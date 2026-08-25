#[cfg(test)]
mod tests {
    use super::super::evolve_tool::*;
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
    async fn test_evolve_tool_create_skill() {
        let dir = tempdir().unwrap();
        let engine = crate::engine::evolution::EvolutionEngine::new(dir.path());
        let tool = EvolveTool::new(std::sync::Arc::new(engine));
        assert_eq!(tool.name(), "Evolve");

        let ctx = make_ctx(dir.path());
        let progress = NoopProgress;

        let input = json!({
            "operation": "create_skill",
            "skill_name": "test_skill",
            "content": "# Test Skill\nDo X then Y.",
            "scope": "project"
        });

        let res = tool.call(input, &ctx, &progress).await;
        assert!(res.is_ok());
        let result = res.unwrap();
        assert_eq!(result.data["created"], true);
    }
}
