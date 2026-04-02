use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio::fs;

/// A discovered MCP server configuration
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct McpServerInfo {
    pub name: String,
    pub command: Option<String>,
    pub args: Vec<String>,
    pub server_type: String, // "stdio", "sse", "http"
    pub url: Option<String>,
    pub disabled: bool,
    pub source: String, // "user", "project", "local"
    pub config_path: String,
}

/// MCP config file format (mcp.json)
#[derive(Debug, Deserialize)]
struct McpJsonConfig {
    #[serde(rename = "mcpServers", default)]
    mcp_servers: HashMap<String, McpServerEntry>,
}

#[derive(Debug, Deserialize)]
struct McpServerEntry {
    command: Option<String>,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    env: HashMap<String, String>,
    #[serde(default)]
    disabled: bool,
    url: Option<String>,
    #[serde(rename = "type")]
    server_type: Option<String>,
}

/// Discover all MCP server configurations from standard locations.
/// Reads from:
///   - ~/.claude/mcp.json (user scope)
///   - .claude/mcp.json in cwd (project scope)
///   - .claude/mcp.local.json in cwd (local scope, gitignored)
pub async fn discover_mcp_servers(cwd: &Path) -> Vec<McpServerInfo> {
    let mut servers = Vec::new();

    // User-level config
    if let Some(home) = dirs_path() {
        let user_config = home.join(".baoclaw").join("mcp.json");
        if let Ok(entries) = load_mcp_config(&user_config, "user").await {
            servers.extend(entries);
        }
    }

    // Project-level config
    let project_config = cwd.join(".baoclaw").join("mcp.json");
    if let Ok(entries) = load_mcp_config(&project_config, "project").await {
        servers.extend(entries);
    }

    // Local config (gitignored)
    let local_config = cwd.join(".baoclaw").join("mcp.local.json");
    if let Ok(entries) = load_mcp_config(&local_config, "local").await {
        servers.extend(entries);
    }

    servers
}

async fn load_mcp_config(
    path: &Path,
    source: &str,
) -> Result<Vec<McpServerInfo>, Box<dyn std::error::Error>> {
    let content = fs::read_to_string(path).await?;
    let config: McpJsonConfig = serde_json::from_str(&content)?;
    let config_path = path.to_string_lossy().to_string();

    let servers = config
        .mcp_servers
        .into_iter()
        .map(|(name, entry)| {
            let server_type = entry
                .server_type
                .unwrap_or_else(|| {
                    if entry.url.is_some() {
                        "sse".to_string()
                    } else {
                        "stdio".to_string()
                    }
                });

            McpServerInfo {
                name,
                command: entry.command,
                args: entry.args,
                server_type,
                url: entry.url,
                disabled: entry.disabled,
                source: source.to_string(),
                config_path: config_path.clone(),
            }
        })
        .collect();

    Ok(servers)
}

fn dirs_path() -> Option<PathBuf> {
    std::env::var("HOME").ok().map(PathBuf::from)
}
