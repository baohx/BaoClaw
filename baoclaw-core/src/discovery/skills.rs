use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::fs;

/// A discovered skill
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SkillInfo {
    pub name: String,
    pub path: String,
    pub source: String, // "user", "project", "managed"
    pub description: Option<String>,
}

/// Scan for skills in standard directories relative to cwd and home.
/// Skills are markdown files in .claude/skills/ directories.
/// Directory format: skill-name/SKILL.md or skill-name.md
pub async fn discover_skills(cwd: &Path) -> Vec<SkillInfo> {
    let mut skills = Vec::new();

    // Project skills: <cwd>/.baoclaw/skills/
    let skills_dir = cwd.join(".baoclaw").join("skills");
    if let Ok(entries) = scan_skills_dir(&skills_dir, "project").await {
        skills.extend(entries);
    }

    // User skills: ~/.baoclaw/skills/
    if let Some(home) = dirs_path() {
        let user_skills = home.join(".baoclaw").join("skills");
        if let Ok(entries) = scan_skills_dir(&user_skills, "user").await {
            skills.extend(entries);
        }

        // Plugin skills: scan ~/.baoclaw/plugins/*/skills/
        let user_plugins = home.join(".baoclaw").join("plugins");
        if let Ok(plugin_skills) = scan_plugin_skills(&user_plugins).await {
            skills.extend(plugin_skills);
        }
    }

    // Project plugin skills: <cwd>/.baoclaw/plugins/*/skills/
    let project_plugins = cwd.join(".baoclaw").join("plugins");
    if let Ok(plugin_skills) = scan_plugin_skills(&project_plugins).await {
        skills.extend(plugin_skills);
    }

    skills
}

/// Scan all plugins in a plugins directory for skills subdirectories.
async fn scan_plugin_skills(plugins_dir: &Path) -> Result<Vec<SkillInfo>, std::io::Error> {
    let mut skills = Vec::new();
    let mut entries = fs::read_dir(plugins_dir).await?;
    while let Some(entry) = entries.next_entry().await? {
        if !entry.file_type().await?.is_dir() { continue; }
        let plugin_name = entry.file_name().to_string_lossy().to_string();
        let skills_subdir = entry.path().join("skills");
        let source = format!("plugin:{}", plugin_name);
        if let Ok(plugin_skills) = scan_skills_dir(&skills_subdir, &source).await {
            skills.extend(plugin_skills);
        }
    }
    Ok(skills)
}

/// Scan a single skills directory for skill files.
async fn scan_skills_dir(dir: &Path, source: &str) -> Result<Vec<SkillInfo>, std::io::Error> {
    let mut skills = Vec::new();

    let mut entries = fs::read_dir(dir).await?;
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        let file_type = entry.file_type().await?;

        if file_type.is_dir() {
            // Directory format: skill-name/SKILL.md
            let skill_file = path.join("SKILL.md");
            if skill_file.exists() {
                let name = entry.file_name().to_string_lossy().to_string();
                let description = read_skill_description(&skill_file).await;
                skills.push(SkillInfo {
                    name,
                    path: skill_file.to_string_lossy().to_string(),
                    source: source.to_string(),
                    description,
                });
            }
        } else if file_type.is_file() {
            // File format: skill-name.md
            let name_os = entry.file_name();
            let name_str = name_os.to_string_lossy();
            if name_str.ends_with(".md") && name_str != "README.md" {
                let name = name_str.trim_end_matches(".md").to_string();
                let description = read_skill_description(&path).await;
                skills.push(SkillInfo {
                    name,
                    path: path.to_string_lossy().to_string(),
                    source: source.to_string(),
                    description,
                });
            }
        }
    }

    Ok(skills)
}

/// Read the first non-frontmatter line as description.
async fn read_skill_description(path: &Path) -> Option<String> {
    let content = fs::read_to_string(path).await.ok()?;
    let mut in_frontmatter = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed == "---" {
            in_frontmatter = !in_frontmatter;
            continue;
        }
        if in_frontmatter {
            // Check for "description:" in frontmatter
            if let Some(desc) = trimmed.strip_prefix("description:") {
                let desc = desc.trim().trim_matches('"').trim_matches('\'');
                if !desc.is_empty() {
                    return Some(desc.to_string());
                }
            }
            continue;
        }
        if !trimmed.is_empty() && !trimmed.starts_with('#') {
            return Some(trimmed.chars().take(120).collect());
        }
        if let Some(heading) = trimmed.strip_prefix('#') {
            return Some(heading.trim().chars().take(120).collect());
        }
    }
    None
}

fn dirs_path() -> Option<PathBuf> {
    std::env::var("HOME").ok().map(PathBuf::from)
}


/// Load all discovered skills and concatenate their full content into a system prompt fragment.
/// Returns None if no skills are found.
pub async fn load_skills_for_prompt(cwd: &Path) -> Option<String> {
    let skills = discover_skills(cwd).await;
    if skills.is_empty() {
        return None;
    }

    let mut parts = Vec::new();
    parts.push("# Loaded Skills\n\nThe following skills are available. Follow their instructions when the user requests the corresponding functionality.\n".to_string());

    for skill in &skills {
        match fs::read_to_string(&skill.path).await {
            Ok(content) => {
                parts.push(format!(
                    "## Skill: {} [source: {}]\n\n{}\n",
                    skill.name, skill.source, content
                ));
            }
            Err(e) => {
                eprintln!("Warning: failed to read skill '{}': {}", skill.name, e);
            }
        }
    }

    if parts.len() <= 1 {
        return None; // Only header, no actual skills loaded
    }

    Some(parts.join("\n"))
}
