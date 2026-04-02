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

    // Project skills: .baoclaw/skills/ in cwd and parent dirs
    let mut dir = cwd.to_path_buf();
    loop {
        let skills_dir = dir.join(".baoclaw").join("skills");
        if let Ok(entries) = scan_skills_dir(&skills_dir, "project").await {
            skills.extend(entries);
        }
        if !dir.pop() {
            break;
        }
    }

    // User skills: ~/.baoclaw/skills/
    if let Some(home) = dirs_path() {
        let user_skills = home.join(".baoclaw").join("skills");
        if let Ok(entries) = scan_skills_dir(&user_skills, "user").await {
            skills.extend(entries);
        }
    }

    skills
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
