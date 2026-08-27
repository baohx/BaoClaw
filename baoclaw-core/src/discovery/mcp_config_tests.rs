#[cfg(test)]
mod tests {
    use super::super::mcp_config::*;
    use tempfile::tempdir;
    use tokio::fs;

    #[tokio::test]
    async fn test_discover_mcp_servers_empty() {
        let dir = tempdir().unwrap();
        let servers = discover_mcp_servers(dir.path()).await;
        // User-level (~/.baoclaw) configs are scanned too, so we cannot
        // assert the whole list is empty. The invariant: an EMPTY project
        // dir contributes zero servers.
        let prefix = dir.path().to_string_lossy().to_string();
        assert!(
            servers.iter().all(|s| !s.config_path.starts_with(&prefix)),
            "empty project dir produced servers: {:?}",
            servers
        );
    }

    #[tokio::test]
    async fn test_discover_mcp_servers_project() {
        let dir = tempdir().unwrap();
        let baoclaw_dir = dir.path().join(".baoclaw");
        fs::create_dir_all(&baoclaw_dir).await.unwrap();

        let config_json = r#"{
            "mcpServers": {
                "test-server": {
                    "command": "node",
                    "args": ["index.js"],
                    "disabled": false,
                    "type": "stdio"
                }
            }
        }"#;
        fs::write(baoclaw_dir.join("mcp.json"), config_json)
            .await
            .unwrap();

        let servers = discover_mcp_servers(dir.path()).await;
        let proj_server = servers.iter().find(|s| s.name == "test-server");
        assert!(proj_server.is_some());
        let s = proj_server.unwrap();
        assert_eq!(s.command.as_deref(), Some("node"));
        assert_eq!(s.args, vec!["index.js"]);
        assert_eq!(s.server_type, "stdio");
        assert_eq!(s.source, "project");
    }

    #[tokio::test]
    async fn test_discover_mcp_servers_local() {
        let dir = tempdir().unwrap();
        let baoclaw_dir = dir.path().join(".baoclaw");
        fs::create_dir_all(&baoclaw_dir).await.unwrap();

        let config_json = r#"{
            "mcpServers": {
                "local-server": {
                    "url": "http://localhost:8080/sse",
                    "type": "sse"
                }
            }
        }"#;
        fs::write(baoclaw_dir.join("mcp.local.json"), config_json)
            .await
            .unwrap();

        let servers = discover_mcp_servers(dir.path()).await;
        let local_server = servers.iter().find(|s| s.name == "local-server");
        assert!(local_server.is_some());
        let s = local_server.unwrap();
        assert_eq!(s.url.as_deref(), Some("http://localhost:8080/sse"));
        assert_eq!(s.server_type, "sse");
        assert_eq!(s.source, "local");
    }
}
