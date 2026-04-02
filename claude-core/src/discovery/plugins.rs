use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use tokio::fs;

/// A discovered plugin
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PluginInfo {
    pub name: String,
    pub version: Option<String>,
    pub description: Option<String>,
    pub path: String,
    pub source: String, // "user", "project"
    pub has_tools: bool,
    pub has_skills: bool,
    pub has_mcp: bool,
}

/// Plugin manifest format (plugin.json)
#[derive(Debug, Deserialize)]
struct PluginManifest {
    name: Option<String>,
    version: Option<String>,
    description: Option<String>,
}

/// Discover plugins from standard directories.
/// Plugins are directories in .claude/plugins/ containing a plugin.json manifest.
pub async fn discover_plugins(cwd: &Path) -> Vec<PluginInfo> {
    let mut plugins = Vec::new();

    // User plugins: ~/.baoclaw/plugins/
    if let Some(home) = dirs_path() {
        let user_plugins = home.join(".baoclaw").join("plugins");
        if let Ok(entries) = scan_plugins_dir(&user_plugins, "user").await {
            plugins.extend(entries);
        }
    }

    // Project plugins: .baoclaw/plugins/ in cwd
    let project_plugins = cwd.join(".baoclaw").join("plugins");
    if let Ok(entries) = scan_plugins_dir(&project_plugins, "project").await {
        plugins.extend(entries);
    }

    plugins
}

async fn scan_plugins_dir(
    dir: &Path,
    source: &str,
) -> Result<Vec<PluginInfo>, std::io::Error> {
    let mut plugins = Vec::new();

    let mut entries = fs::read_dir(dir).await?;
    while let Some(entry) = entries.next_entry().await? {
        if !entry.file_type().await?.is_dir() {
            continue;
        }

        let plugin_dir = entry.path();
        let manifest_path = plugin_dir.join("plugin.json");

        // Try to read manifest
        let (name, version, description) = if let Ok(content) =
            fs::read_to_string(&manifest_path).await
        {
            if let Ok(manifest) = serde_json::from_str::<PluginManifest>(&content) {
                (
                    manifest.name.unwrap_or_else(|| {
                        entry.file_name().to_string_lossy().to_string()
                    }),
                    manifest.version,
                    manifest.description,
                )
            } else {
                (entry.file_name().to_string_lossy().to_string(), None, None)
            }
        } else {
            // No manifest, use directory name
            (entry.file_name().to_string_lossy().to_string(), None, None)
        };

        // Check for sub-features
        let has_tools = plugin_dir.join("tools").exists();
        let has_skills = plugin_dir.join("skills").exists();
        let has_mcp = plugin_dir.join("mcp.json").exists();

        plugins.push(PluginInfo {
            name,
            version,
            description,
            path: plugin_dir.to_string_lossy().to_string(),
            source: source.to_string(),
            has_tools,
            has_skills,
            has_mcp,
        });
    }

    Ok(plugins)
}

fn dirs_path() -> Option<PathBuf> {
    std::env::var("HOME").ok().map(PathBuf::from)
}
