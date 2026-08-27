#[cfg(test)]
mod tests {
    use super::super::plugins::*;
    use tempfile::tempdir;
    use tokio::fs;

    #[tokio::test]
    async fn test_discover_plugins_empty() {
        let dir = tempdir().unwrap();
        let plugins = discover_plugins(dir.path()).await;
        // User-level (~/.baoclaw) dirs are scanned too, so we cannot assert
        // the whole list is empty. The invariant: an EMPTY project dir
        // contributes zero plugins.
        let prefix = dir.path().to_string_lossy().to_string();
        assert!(
            plugins.iter().all(|p| !p.path.starts_with(&prefix)),
            "empty project dir produced plugins: {:?}",
            plugins
        );
    }

    #[tokio::test]
    async fn test_discover_plugins_project() {
        let dir = tempdir().unwrap();
        let plugin_dir = dir
            .path()
            .join(".baoclaw")
            .join("plugins")
            .join("my-plugin");
        fs::create_dir_all(&plugin_dir).await.unwrap();

        let manifest = r#"{
            "name": "my-plugin",
            "version": "1.0.0",
            "description": "A test plugin"
        }"#;
        fs::write(plugin_dir.join("plugin.json"), manifest)
            .await
            .unwrap();

        let plugins = discover_plugins(dir.path()).await;
        let p = plugins.iter().find(|p| p.name == "my-plugin");
        assert!(p.is_some());
        let plug = p.unwrap();
        assert_eq!(plug.version.as_deref(), Some("1.0.0"));
        assert_eq!(plug.description.as_deref(), Some("A test plugin"));
        assert_eq!(plug.source, "project");
    }
}
