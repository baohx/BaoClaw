use serde_json::Value;
use std::path::Path;

use crate::api::client::CreateMessageRequest;
use crate::engine::query_engine::{CachedRule, QueryLoopConfig, ThinkingConfig};
use crate::engine::query_loop::validate_and_fix_tool_messages;
use crate::models::message::{ContentBlock, Message, MessageContent};


/// Load project instructions from BAOCLAW.md files.
///
/// Scans `.baoclaw/BAOCLAW.md` first, then `BAOCLAW.md` in the given directory.
/// Returns the content of the first found non-empty file, or None.
pub fn load_project_instructions(cwd: &Path) -> Option<String> {
    let paths = [
        cwd.join(".baoclaw").join("BAOCLAW.md"),
        cwd.join("BAOCLAW.md"),
    ];
    for p in &paths {
        if let Ok(content) = std::fs::read_to_string(p) {
            if !content.trim().is_empty() {
                return Some(content);
            }
        }
    }
    None
}

/// A parsed rule file from `.baoclaw/rules/*.md`.
pub struct RuleFile {
    /// Rule content (with YAML frontmatter stripped).
    content: String,
    /// Optional glob pattern from frontmatter `paths` field.
    paths_pattern: Option<String>,
}

/// Load rules from `.baoclaw/rules/*.md`, optionally filtering by `recent_file_paths`.
///
/// Rules without a `paths` frontmatter field are loaded unconditionally.
/// Rules with `paths` are only included when at least one entry in
/// `recent_file_paths` matches the glob pattern.
pub fn load_rules_with_paths(cwd: &Path, recent_file_paths: &[String]) -> Vec<String> {
    let rules_dir = cwd.join(".baoclaw").join("rules");
    let entries = match std::fs::read_dir(&rules_dir) {
        Ok(rd) => rd,
        Err(_) => return Vec::new(),
    };

    let mut matched_rules: Vec<String> = Vec::new();

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }

        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let rule = parse_rule_file(&content);
        let should_include = match &rule.paths_pattern {
            None => true,
            Some(pattern) => {
                // Use glob matching: include if any recent file path matches
                match glob::Pattern::new(pattern) {
                    Ok(glob_pattern) => {
                        recent_file_paths.iter().any(|fp| {
                            // Try matching against the full path or just the filename
                            glob_pattern.matches(fp) || glob_pattern.matches(std::path::Path::new(fp).file_name().and_then(|n| n.to_str()).unwrap_or(""))
                        })
                    }
                    Err(e) => {
                        eprintln!("Warning: invalid glob pattern '{}' in {}: {}", pattern, path.display(), e);
                        // If pattern is invalid, include the rule anyway
                        true
                    }
                }
            }
        };

        if should_include && !rule.content.trim().is_empty() {
            matched_rules.push(format!(
                "# Rule: {}\n\n{}",
                path.file_name().and_then(|n| n.to_str()).unwrap_or("unknown"),
                rule.content
            ));
        }
    }

    matched_rules
}

/// Parse a rule file, extracting YAML frontmatter if present.
///
/// Supports `---` delimited frontmatter with a `paths` field:
/// ```markdown
/// ---
/// paths: "src/**/*.rs"
/// ---
/// Rule content here.
/// ```
pub fn parse_rule_file(content: &str) -> RuleFile {
    let trimmed = content.trim();

    // Check for YAML frontmatter
    if trimmed.starts_with("---") {
        // Find closing ---
        if let Some(rest) = trimmed.get(3..) {
            if let Some(end_idx) = rest.find("---") {
                let frontmatter = &rest[..end_idx];
                let body = rest[end_idx + 3..].trim();

                // Parse paths from frontmatter (simple line-based parsing)
                let paths_pattern = frontmatter
                    .lines()
                    .find_map(|line| {
                        let line = line.trim();
                        if line.starts_with("paths:") || line.starts_with("paths :") {
                            let value = line.splitn(2, ':').nth(1)?.trim();
                            // Strip quotes if present
                            let value = value.trim_matches('"').trim_matches('\'');
                            if value.is_empty() {
                                None
                            } else {
                                Some(value.to_string())
                            }
                        } else {
                            None
                        }
                    });

                return RuleFile {
                    content: body.to_string(),
                    paths_pattern,
                };
            }
        }
    }

    // No frontmatter
    RuleFile {
        content: trimmed.to_string(),
        paths_pattern: None,
    }
}

/// Load all rule files from `.baoclaw/rules/*.md` into cached structures.
/// This is called once in `QueryEngine::new()` and the results are reused
/// across turns, avoiding repeated file I/O.
pub fn load_all_rule_files(cwd: &Path) -> Vec<CachedRule> {
    let rules_dir = cwd.join(".baoclaw").join("rules");
    let entries = match std::fs::read_dir(&rules_dir) {
        Ok(rd) => rd,
        Err(_) => return Vec::new(),
    };

    let mut rules = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let rule = parse_rule_file(&content);
        let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("unknown").to_string();
        rules.push(CachedRule {
            filename,
            content: rule.content,
            paths_pattern: rule.paths_pattern,
        });
    }
    rules
}

/// Filter cached rules against recent file paths using glob matching.
/// Replaces `load_rules_with_paths` for in-memory cached rules.
pub fn filter_cached_rules(cached: &[CachedRule], recent_file_paths: &[String]) -> Vec<String> {
    cached.iter()
        .filter(|rule| {
            match &rule.paths_pattern {
                None => true,
                Some(pattern) => {
                    match glob::Pattern::new(pattern) {
                        Ok(glob_pattern) => {
                            recent_file_paths.iter().any(|fp| {
                                glob_pattern.matches(fp) || glob_pattern.matches(
                                    std::path::Path::new(fp).file_name().and_then(|n| n.to_str()).unwrap_or("")
                                )
                            })
                        }
                        Err(_) => true, // Invalid pattern → include anyway
                    }
                }
            }
        })
        .filter(|rule| !rule.content.trim().is_empty())
        .map(|rule| format!("# Rule: {}\n\n{}", rule.filename, rule.content))
        .collect()
}

/// Extract file paths mentioned in the most recent N messages.
///
/// Looks for file_path, file, path, pattern, cwd, and directory fields in
/// tool inputs and text content.
pub fn extract_recent_file_paths(messages: &[Message], max_messages: usize) -> Vec<String> {
    let start = messages.len().saturating_sub(max_messages);
    let mut paths = Vec::new();

    for msg in &messages[start..] {
        match &msg.content {
            MessageContent::User { message, .. } => {
                // Look for file_path in tool_result content
                if let Value::Array(blocks) = &message.content {
                    for block in blocks {
                        if let Some(content) = block.get("content").and_then(|c| c.as_str()) {
                            // Extract file paths that appear in tool results
                            for line in content.lines().take(50) {
                                if line.contains('/') || line.contains("\\.") {
                                    paths.push(line.trim().to_string());
                                }
                            }
                        }
                    }
                }
                if let Value::String(text) = &message.content {
                    // Simple heuristic: extract path-like strings from text
                    for word in text.split_whitespace() {
                        if (word.contains('/') || word.contains(".rs") || word.contains(".ts")
                            || word.contains(".js") || word.contains(".py")
                            || word.contains(".md") || word.contains(".toml"))
                            && word.len() > 5 && word.len() < 300
                        {
                            paths.push(word.to_string());
                        }
                    }
                }
            }
            MessageContent::Assistant { message, .. } => {
                for block in &message.content {
                    if let ContentBlock::ToolUse { input, .. } = block {
                        // Extract file_path from tool inputs
                        if let Some(fp) = input.get("file_path").and_then(|v| v.as_str()) {
                            paths.push(fp.to_string());
                        }
                        if let Some(p) = input.get("path").and_then(|v| v.as_str()) {
                            paths.push(p.to_string());
                        }
                        if let Some(p) = input.get("pattern").and_then(|v| v.as_str()) {
                            paths.push(p.to_string());
                        }
                    }
                }
            }
            _ => {}
        }
    }

    // Deduplicate while preserving order
    let mut seen = std::collections::HashSet::new();
    paths.retain(|p| seen.insert(p.clone()));

    // Limit to reasonable count
    paths.truncate(50);
    paths
}

/// Build an API request from the current messages and config.
pub fn build_api_request(messages: &[Message], config: &QueryLoopConfig) -> CreateMessageRequest {
    // First validate and fix tool_use/tool_result pairing
    let validated_messages = validate_and_fix_tool_messages(messages);

    // Convert messages to API format
    let mut api_messages: Vec<Value> = validated_messages.iter().filter_map(|msg| {
        match &msg.content {
            MessageContent::User { message, .. } => {
                // Skip empty user messages
                let is_empty = match &message.content {
                    Value::String(s) => s.trim().is_empty(),
                    Value::Array(arr) => arr.is_empty(),
                    _ => message.content.is_null(),
                };
                if is_empty {
                    eprintln!("Skipping empty user message");
                    return None;
                }
                Some(serde_json::json!({
                    "role": message.role,
                    "content": message.content,
                }))
            }
            MessageContent::Assistant { message, .. } => {
                // Skip empty assistant messages
                if message.content.is_empty() {
                    eprintln!("Skipping empty assistant message");
                    return None;
                }
                // Also check if all content blocks are empty
                let has_content = message.content.iter().any(|block| {
                    match block {
                        ContentBlock::Text { text } => !text.trim().is_empty(),
                        ContentBlock::Thinking { thinking } => !thinking.trim().is_empty(),
                        ContentBlock::ToolUse { .. } => true,
                        _ => false,
                    }
                });
                if !has_content {
                    eprintln!("Skipping assistant message with no valid content");
                    return None;
                }
                let content_value = serde_json::to_value(&message.content).unwrap_or(Value::Array(vec![]));
                Some(serde_json::json!({
                    "role": message.role,
                    "content": content_value,
                }))
            }
            _ => None,
        }
    }).collect();

    // Inject dynamic <system-reminder> into the last user message to avoid
    // invalidating the cached system prompt prefix.  Git status, session
    // memory, and other per-turn information goes here.
    if let Some(reminder) = build_dynamic_reminder(config) {
        if let Some(last_msg) = api_messages.last_mut() {
            // Append the reminder to the existing user message content
            if let Some(content) = last_msg.get_mut("content") {
                match content {
                    Value::String(s) => {
                        *s = format!("{}\n\n{}", s, reminder);
                    }
                    Value::Array(blocks) => {
                        blocks.push(serde_json::json!({
                            "type": "text",
                            "text": reminder,
                        }));
                    }
                    _ => {
                        // Fallback: replace content with a string containing the reminder
                        *content = Value::String(format!("{}\n\n{}", content, reminder));
                    }
                }
            }
        }
    }

    // Use frozen system prompt if available (maximizes cache hit rate)
    let system = if let Some(ref frozen) = config.frozen_system_prompt {
        Some(frozen.clone())
    } else {
        build_system_prompt(config)
    };

    // Use frozen tools list if available (same caching benefit)
    let tools = if let Some(ref frozen) = config.frozen_tools {
        Some(frozen.clone())
    } else {
        build_tools_list(config)
    };

    CreateMessageRequest {
        model: config.model.clone(),
        messages: api_messages,
        system,
        tools,
        max_tokens: 16384,
        stream: true,
        thinking: match &config.thinking_config {
            ThinkingConfig::Disabled => None,
            ThinkingConfig::Adaptive => Some(serde_json::json!({
                "type": "enabled",
                "budget_tokens": 10240
            })),
            ThinkingConfig::Enabled { budget_tokens } => Some(serde_json::json!({
                "type": "enabled",
                "budget_tokens": budget_tokens
            })),
        },
        metadata: None,
    }
}

/// Build the tools list deterministically — called once and frozen for the session.
///
/// Tool order is part of the cached prefix, so non-deterministic iteration
/// (e.g. HashMap-based) would break caching.
pub fn build_tools_list(config: &QueryLoopConfig) -> Option<Vec<Value>> {
    if config.tools.is_empty() {
        return None;
    }
    let mut tool_list: Vec<Value> = config.tools.iter().map(|t| {
        if t.is_deferred() {
            serde_json::json!({
                "name": t.name(),
                "description": t.short_description(),
                "defer_loading": true,
            })
        } else {
            let schema = t.input_schema();
            serde_json::json!({
                "name": t.name(),
                "description": t.prompt(),
                "input_schema": schema,
            })
        }
    }).collect();
    tool_list.sort_by(|a, b| {
        let name_a = a.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let name_b = b.get("name").and_then(|v| v.as_str()).unwrap_or("");
        name_a.cmp(name_b)
    });
    if let Some(last_tool) = tool_list.last_mut() {
        last_tool.as_object_mut().map(|obj| {
            obj.insert(
                "cache_control".to_string(),
                serde_json::json!({ "type": "ephemeral" }),
            );
        });
    }
    Some(tool_list)
}

/// Build the system prompt from config — **static parts only**.
///
/// Prompt Caching works via prefix matching: any change to the system prompt
/// invalidates the entire cached prefix.  Therefore, only content that is
/// stable across turns is placed here.  Dynamic information (git status,
/// session memory) is injected via `<system-reminder>` user messages instead.
///
/// Order (stable → volatile):
///   1. Core system prompt / custom prompt      ← never changes in-session
///   2. Working directory                        ← never changes in-session
///   3. Project instructions (BAOCLAW.md)        ← rarely changes
///   4. Project rules (.baoclaw/rules/)          ← rarely changes
///   5. Append system prompt                     ← rarely changes
pub fn build_system_prompt(config: &QueryLoopConfig) -> Option<Vec<Value>> {
    let mut parts: Vec<String> = Vec::new();

    // 1. Core system prompt
    if let Some(custom) = &config.custom_system_prompt {
        parts.push(custom.clone());
    } else {
        parts.push("You are a helpful AI coding assistant.".to_string());
    }

    // 2. Current working directory (stable within a session)
    parts.push(format!(
        "Current working directory: {}\n\nWhen the user asks to display or show a file's content, output the full content directly in your response. Do not summarize or describe the file — show the actual text.",
        config.cwd.display()
    ));

    // 3. Project instructions from BAOCLAW.md (rarely changes mid-session)
    if let Some(instructions) = &config.project_instructions {
        parts.push(format!(
            "# Project Instructions (from BAOCLAW.md)\n\n{}",
            instructions
        ));
    }

    // 4. Project rules from .baoclaw/rules/*.md (path-filtered from cache)
    {
        let recent_paths = extract_recent_file_paths(&config.recent_messages_for_rules, 10);
        let rules = filter_cached_rules(&config.cached_rules_raw, &recent_paths);
        if !rules.is_empty() {
            parts.push(format!(
                "# Project Rules (from .baoclaw/rules/)\n\n{}",
                rules.join("\n\n")
            ));
        }
    }

    // 5. Append system prompt
    if let Some(append) = &config.append_system_prompt {
        parts.push(append.clone());
    }

    if parts.is_empty() {
        None
    } else {
        let combined = parts.join("\n\n");
        // Mark the static system prompt with cache_control so the API caches it.
        Some(vec![serde_json::json!({
            "type": "text",
            "text": combined,
            "cache_control": { "type": "ephemeral" },
        })])
    }
}

/// Build a `<system-reminder>` user message containing **dynamic** information
/// that changes between turns (git status, session memory, etc.).
///
/// This content is kept out of the system prompt so that the cached prefix
/// remains stable.  The reminder is appended as a user message — the model
/// still sees it, but it doesn't invalidate the prompt cache.
pub fn build_dynamic_reminder(config: &QueryLoopConfig) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();

    // Git status — changes every turn as files are edited
    if let Some(git_info) = &config.git_info {
        let mut git_parts: Vec<String> = Vec::new();
        if let Some(branch) = &git_info.branch {
            git_parts.push(format!("Current git branch: {}", branch));
        }
        if git_info.has_changes {
            let mut change_lines: Vec<String> = Vec::new();
            if !git_info.staged_files.is_empty() {
                change_lines.push(format!("Staged: {}", git_info.staged_files.join(", ")));
            }
            if !git_info.modified_files.is_empty() {
                change_lines.push(format!("Modified: {}", git_info.modified_files.join(", ")));
            }
            if !git_info.untracked_files.is_empty() {
                change_lines.push(format!("Untracked: {}", git_info.untracked_files.join(", ")));
            }
            git_parts.push(format!("Changed files:\n{}", change_lines.join("\n")));
        }
        if !git_parts.is_empty() {
            parts.push(format!("# Git Status\n\n{}", git_parts.join("\n")));
        }
    }

    // Session memory (rolling summary) — updated after compaction
    if let Some(sm) = &config.session_memory {
        let memory = sm.get();
        if !memory.is_empty() {
            parts.push(format!("# Session Memory\n\n{}", memory));
        }
    }

    if parts.is_empty() {
        None
    } else {
        Some(format!("<system-reminder>\n{}\n</system-reminder>", parts.join("\n\n")))
    }
}
