//! User profile management — USER.md persistent user profile.
//!
//! Maintains a structured user profile at `~/.baoclaw/USER.md` that captures
//! user preferences, coding style, common patterns, and interaction history.
//! Loaded at session start and injected into the system prompt.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Mutex;

// ── Data Structures ──────────────────────────────────────────────────────────

/// User profile sections
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UserProfile {
    /// User's preferred name (if set)
    pub name: Option<String>,
    /// Preferred language for responses
    pub preferred_language: Option<String>,
    /// Coding style preferences
    pub coding_style: Vec<String>,
    /// Common workflows/patterns
    pub workflows: Vec<String>,
    /// Tool preferences (e.g., preferred editor, test framework)
    pub tool_preferences: Vec<(String, String)>,
    /// Project-specific preferences
    pub project_preferences: Vec<ProjectPref>,
    /// Interaction statistics
    pub stats: ProfileStats,
    /// Custom instructions (free-form)
    pub custom_instructions: String,
    /// Last updated timestamp
    pub updated_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProjectPref {
    pub path: String,
    pub notes: Vec<String>,
    pub model_preference: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProfileStats {
    /// Total sessions
    pub total_sessions: u64,
    /// Total turns across all sessions
    pub total_turns: u64,
    /// Total cost USD
    pub total_cost_usd: f64,
    /// Most used tools (tool_name, count)
    pub top_tools: Vec<(String, u32)>,
    /// Most common task types
    pub common_tasks: Vec<(String, u32)>,
    /// Average session duration in seconds
    pub avg_session_duration: f64,
}

impl Default for ProfileStats {
    fn default() -> Self {
        Self {
            total_sessions: 0,
            total_turns: 0,
            total_cost_usd: 0.0,
            top_tools: Vec::new(),
            common_tasks: Vec::new(),
            avg_session_duration: 0.0,
        }
    }
}

impl Default for UserProfile {
    fn default() -> Self {
        Self {
            name: None,
            preferred_language: None,
            coding_style: Vec::new(),
            workflows: Vec::new(),
            tool_preferences: Vec::new(),
            project_preferences: Vec::new(),
            stats: ProfileStats::default(),
            custom_instructions: String::new(),
            updated_at: chrono::Utc::now().to_rfc3339(),
        }
    }
}

// ── Session stats (for merge) ────────────────────────────────────────────────

pub struct SessionStats {
    pub turns: u64,
    pub cost_usd: f64,
    pub tools_used: Vec<(String, u32)>,
    pub duration_secs: f64,
    pub task_types: Vec<String>,
}

// ── Profile Manager ──────────────────────────────────────────────────────────

/// Manages the USER.md profile file.
pub struct UserProfileManager {
    file_path: PathBuf,
    profile: Mutex<UserProfile>,
}

// ── Parsing helpers ──────────────────────────────────────────────────────────

/// Parse USER.md content into a `UserProfile`. Failures fall back to defaults.
fn parse_user_md(content: &str) -> UserProfile {
    let mut profile = UserProfile::default();

    // Split into sections by `## Section` headers
    let mut sections: Vec<(&str, Vec<&str>)> = Vec::new();
    let mut current_section = "";
    let mut current_lines: Vec<&str> = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("## ") {
            if !current_section.is_empty() || !current_lines.is_empty() {
                sections.push((current_section, current_lines.clone()));
            }
            current_section = trimmed.trim_start_matches('#').trim();
            current_lines.clear();
        } else {
            current_lines.push(line);
        }
    }
    // Push the last section
    if !current_section.is_empty() || !current_lines.is_empty() {
        sections.push((current_section, current_lines));
    }

    for (section_name, lines) in sections {
        match section_name {
            "Personal" => parse_personal(&lines, &mut profile),
            "Coding Style" => parse_list_section(&lines, &mut profile.coding_style),
            "Workflows" => parse_list_section(&lines, &mut profile.workflows),
            "Tool Preferences" => parse_tool_prefs(&lines, &mut profile.tool_preferences),
            "Projects" => parse_projects(&lines, &mut profile.project_preferences),
            "Stats" => parse_stats(&lines, &mut profile.stats),
            "Custom Instructions" => {
                let text: Vec<String> = lines
                    .iter()
                    .map(|l| l.to_string())
                    .collect();
                profile.custom_instructions = text.join("\n").trim().to_string();
            }
            _ => {} // Ignore unknown sections (including the top-level "# User Profile")
        }
    }

    profile
}

fn parse_personal(lines: &[&str], profile: &mut UserProfile) {
    for line in lines {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("- Name:") {
            let val = rest.trim().to_string();
            if !val.is_empty() {
                profile.name = Some(val);
            }
        } else if let Some(rest) = trimmed.strip_prefix("- Preferred Language:") {
            let val = rest.trim().to_string();
            if !val.is_empty() {
                profile.preferred_language = Some(val);
            }
        }
    }
}

fn parse_list_section(lines: &[&str], target: &mut Vec<String>) {
    for line in lines {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(item) = trimmed.strip_prefix("- ") {
            let val = item.trim().to_string();
            if !val.is_empty() {
                target.push(val);
            }
        }
    }
}

fn parse_tool_prefs(lines: &[&str], target: &mut Vec<(String, String)>) {
    for line in lines {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(item) = trimmed.strip_prefix("- ") {
            // Format: "tool_name: preference"
            if let Some((k, v)) = item.split_once(':') {
                let key = k.trim().to_string();
                let val = v.trim().to_string();
                if !key.is_empty() {
                    target.push((key, val));
                }
            }
        }
    }
}

fn parse_projects(lines: &[&str], target: &mut Vec<ProjectPref>) {
    let mut current: Option<ProjectPref> = None;

    for line in lines {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        // `### /path/to/project` — sub-header for a project
        if let Some(path) = trimmed.strip_prefix("### ") {
            // Save previous project if any
            if let Some(prev) = current.take() {
                target.push(prev);
            }
            current = Some(ProjectPref {
                path: path.trim().to_string(),
                notes: Vec::new(),
                model_preference: None,
            });
            continue;
        }
        // Lines under a project sub-header
        if let Some(ref mut proj) = current {
            if let Some(item) = trimmed.strip_prefix("- ") {
                let val = item.trim();
                if let Some(model) = val.strip_prefix("Model:") {
                    proj.model_preference = Some(model.trim().to_string());
                } else {
                    proj.notes.push(val.to_string());
                }
            }
        }
    }
    // Don't forget the last project
    if let Some(proj) = current.take() {
        target.push(proj);
    }
}

fn parse_stats(lines: &[&str], stats: &mut ProfileStats) {
    for line in lines {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("- Sessions:") {
            stats.total_sessions = rest.trim().parse().unwrap_or(0);
        } else if let Some(rest) = trimmed.strip_prefix("- Total Turns:") {
            stats.total_turns = rest.trim().parse().unwrap_or(0);
        } else if let Some(rest) = trimmed.strip_prefix("- Total Cost:") {
            // Format may be "$12.34" or just "12.34"
            let cleaned = rest.trim().trim_start_matches('$');
            stats.total_cost_usd = cleaned.parse().unwrap_or(0.0);
        } else if let Some(rest) = trimmed.strip_prefix("- Top Tools:") {
            stats.top_tools = parse_counted_list(rest.trim());
        } else if let Some(rest) = trimmed.strip_prefix("- Common Tasks:") {
            stats.common_tasks = parse_counted_list(rest.trim());
        } else if let Some(rest) = trimmed.strip_prefix("- Avg Duration:") {
            let cleaned = rest.trim().trim_end_matches('s');
            stats.avg_session_duration = cleaned.parse().unwrap_or(0.0);
        }
    }
}

/// Parse a counted list like "Bash(200), FileEdit(150)"
fn parse_counted_list(s: &str) -> Vec<(String, u32)> {
    s.split(',')
        .filter_map(|item| {
            let item = item.trim();
            if item.is_empty() {
                return None;
            }
            // Find the last `(N)` pattern
            let open = item.rfind('(')?;
            let close = item.rfind(')')?;
            if open >= close {
                return None;
            }
            let name = item[..open].trim().to_string();
            let count: u32 = item[open + 1..close].parse().ok()?;
            if name.is_empty() {
                None
            } else {
                Some((name, count))
            }
        })
        .collect()
}

// ── Serialization helpers ────────────────────────────────────────────────────

fn profile_to_markdown(profile: &UserProfile) -> String {
    let mut out = String::new();

    out.push_str("# User Profile\n\n");

    // Personal
    out.push_str("## Personal\n");
    out.push_str(&format!(
        "- Name: {}\n",
        profile.name.as_deref().unwrap_or("")
    ));
    out.push_str(&format!(
        "- Preferred Language: {}\n",
        profile.preferred_language.as_deref().unwrap_or("")
    ));
    out.push('\n');

    // Coding Style
    out.push_str("## Coding Style\n");
    for style in &profile.coding_style {
        out.push_str(&format!("- {}\n", style));
    }
    out.push('\n');

    // Workflows
    out.push_str("## Workflows\n");
    for wf in &profile.workflows {
        out.push_str(&format!("- {}\n", wf));
    }
    out.push('\n');

    // Tool Preferences
    out.push_str("## Tool Preferences\n");
    for (tool, pref) in &profile.tool_preferences {
        out.push_str(&format!("- {}: {}\n", tool, pref));
    }
    out.push('\n');

    // Projects
    out.push_str("## Projects\n");
    for proj in &profile.project_preferences {
        out.push_str(&format!("### {}\n", proj.path));
        for note in &proj.notes {
            out.push_str(&format!("- {}\n", note));
        }
        if let Some(ref model) = proj.model_preference {
            out.push_str(&format!("- Model: {}\n", model));
        }
        out.push('\n');
    }

    // Stats
    out.push_str("## Stats\n");
    out.push_str(&format!("- Sessions: {}\n", profile.stats.total_sessions));
    out.push_str(&format!("- Total Turns: {}\n", profile.stats.total_turns));
    out.push_str(&format!(
        "- Total Cost: ${:.2}\n",
        profile.stats.total_cost_usd
    ));
    let top_tools_str: Vec<String> = profile
        .stats
        .top_tools
        .iter()
        .map(|(n, c)| format!("{}({})", n, c))
        .collect();
    out.push_str(&format!("- Top Tools: {}\n", top_tools_str.join(", ")));
    let tasks_str: Vec<String> = profile
        .stats
        .common_tasks
        .iter()
        .map(|(n, c)| format!("{}({})", n, c))
        .collect();
    out.push_str(&format!("- Common Tasks: {}\n", tasks_str.join(", ")));
    out.push_str(&format!(
        "- Avg Duration: {:.0}s\n",
        profile.stats.avg_session_duration
    ));
    out.push('\n');

    // Custom Instructions
    out.push_str("## Custom Instructions\n");
    if !profile.custom_instructions.is_empty() {
        out.push_str(&profile.custom_instructions);
        out.push('\n');
    }

    out
}

// ── UserProfileManager impl ──────────────────────────────────────────────────

impl UserProfileManager {
    /// Create a new manager, loading existing profile or creating default.
    pub fn new() -> Self {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        let dir = PathBuf::from(&home).join(".baoclaw");
        let file_path = dir.join("USER.md");

        let profile = if file_path.exists() {
            match std::fs::read_to_string(&file_path) {
                Ok(content) => parse_user_md(&content),
                Err(_) => UserProfile::default(),
            }
        } else {
            UserProfile::default()
        };

        eprintln!(
            "Loaded user profile from {} (name={:?})",
            file_path.display(),
            profile.name
        );

        Self {
            file_path,
            profile: Mutex::new(profile),
        }
    }

    /// Get current profile (clone)
    pub fn get(&self) -> UserProfile {
        self.profile.lock().unwrap().clone()
    }

    /// Update user's name
    pub fn update_name(&self, name: String) {
        let mut p = self.profile.lock().unwrap();
        p.name = Some(name);
        p.updated_at = chrono::Utc::now().to_rfc3339();
    }

    /// Update preferred language
    pub fn update_language(&self, lang: String) {
        let mut p = self.profile.lock().unwrap();
        p.preferred_language = Some(lang);
        p.updated_at = chrono::Utc::now().to_rfc3339();
    }

    /// Add a coding style preference (no duplicates)
    pub fn add_coding_style(&self, style: String) {
        let mut p = self.profile.lock().unwrap();
        if !p.coding_style.contains(&style) {
            p.coding_style.push(style);
        }
        p.updated_at = chrono::Utc::now().to_rfc3339();
    }

    /// Add a workflow (no duplicates)
    pub fn add_workflow(&self, workflow: String) {
        let mut p = self.profile.lock().unwrap();
        if !p.workflows.contains(&workflow) {
            p.workflows.push(workflow);
        }
        p.updated_at = chrono::Utc::now().to_rfc3339();
    }

    /// Add or update a tool preference
    pub fn add_tool_preference(&self, tool: String, pref: String) {
        let mut p = self.profile.lock().unwrap();
        if let Some(entry) = p.tool_preferences.iter_mut().find(|(t, _)| t == &tool) {
            entry.1 = pref;
        } else {
            p.tool_preferences.push((tool, pref));
        }
        p.updated_at = chrono::Utc::now().to_rfc3339();
    }

    /// Add a note to a project (creates project entry if needed)
    pub fn add_project_note(&self, project_path: String, note: String) {
        let mut p = self.profile.lock().unwrap();
        if let Some(proj) = p
            .project_preferences
            .iter_mut()
            .find(|proj| proj.path == project_path)
        {
            if !proj.notes.contains(&note) {
                proj.notes.push(note);
            }
        } else {
            p.project_preferences.push(ProjectPref {
                path: project_path,
                notes: vec![note],
                model_preference: None,
            });
        }
        p.updated_at = chrono::Utc::now().to_rfc3339();
    }

    /// Update custom instructions (replaces entirely)
    pub fn update_custom_instructions(&self, instructions: String) {
        let mut p = self.profile.lock().unwrap();
        p.custom_instructions = instructions;
        p.updated_at = chrono::Utc::now().to_rfc3339();
    }

    /// Merge session stats into profile stats
    pub fn merge_session_stats(&self, session: &SessionStats) {
        let mut p = self.profile.lock().unwrap();

        // Running average for session duration
        let prev_total = p.stats.total_sessions as f64;
        let new_total = prev_total + 1.0;
        p.stats.avg_session_duration = if prev_total > 0.0 {
            (p.stats.avg_session_duration * prev_total + session.duration_secs) / new_total
        } else {
            session.duration_secs
        };

        p.stats.total_sessions += 1;
        p.stats.total_turns += session.turns;
        p.stats.total_cost_usd += session.cost_usd;

        // Merge tool counts
        for (tool, count) in &session.tools_used {
            if let Some(entry) = p.stats.top_tools.iter_mut().find(|(t, _)| t == tool) {
                entry.1 += count;
            } else {
                p.stats.top_tools.push((tool.clone(), *count));
            }
        }
        // Sort by count descending, keep top 10
        p.stats.top_tools.sort_by(|a, b| b.1.cmp(&a.1));
        p.stats.top_tools.truncate(10);

        // Merge task types
        for task in &session.task_types {
            if let Some(entry) = p.stats.common_tasks.iter_mut().find(|(t, _)| t == task) {
                entry.1 += 1;
            } else {
                p.stats.common_tasks.push((task.clone(), 1));
            }
        }
        p.stats.common_tasks.sort_by(|a, b| b.1.cmp(&a.1));
        p.stats.common_tasks.truncate(10);

        p.updated_at = chrono::Utc::now().to_rfc3339();
    }

    /// Persist to disk (markdown format)
    pub fn save(&self) {
        let p = self.profile.lock().unwrap();
        let md = profile_to_markdown(&p);

        // Ensure parent directory exists
        if let Some(parent) = self.file_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        if let Err(e) = std::fs::write(&self.file_path, &md) {
            eprintln!("Failed to save USER.md: {}", e);
        }
    }

    /// Build a system prompt fragment from the profile.
    /// Returns `None` if the profile is essentially empty (no name, no styles,
    /// no workflows, no custom instructions).
    pub fn build_prompt_fragment(&self) -> Option<String> {
        let p = self.profile.lock().unwrap();

        let is_empty = p.name.is_none()
            && p.preferred_language.is_none()
            && p.coding_style.is_empty()
            && p.workflows.is_empty()
            && p.tool_preferences.is_empty()
            && p.project_preferences.is_empty()
            && p.custom_instructions.trim().is_empty();

        if is_empty {
            return None;
        }

        let mut parts = Vec::new();
        parts.push("# User Profile\n".to_string());

        if let Some(ref name) = p.name {
            parts.push(format!("The user's name is **{}**.", name));
        }
        if let Some(ref lang) = p.preferred_language {
            parts.push(format!("Respond in **{}** unless the user writes in another language.", lang));
        }
        if !p.coding_style.is_empty() {
            parts.push("\n## Coding Style Preferences".to_string());
            for s in &p.coding_style {
                parts.push(format!("- {}", s));
            }
        }
        if !p.workflows.is_empty() {
            parts.push("\n## Common Workflows".to_string());
            for w in &p.workflows {
                parts.push(format!("- {}", w));
            }
        }
        if !p.tool_preferences.is_empty() {
            parts.push("\n## Tool Preferences".to_string());
            for (tool, pref) in &p.tool_preferences {
                parts.push(format!("- {}: {}", tool, pref));
            }
        }
        if !p.project_preferences.is_empty() {
            parts.push("\n## Project Notes".to_string());
            for proj in &p.project_preferences {
                parts.push(format!("### {}", proj.path));
                for note in &proj.notes {
                    parts.push(format!("- {}", note));
                }
            }
        }
        if !p.custom_instructions.trim().is_empty() {
            parts.push("\n## Custom Instructions".to_string());
            parts.push(p.custom_instructions.clone());
        }

        Some(parts.join("\n"))
    }
}
