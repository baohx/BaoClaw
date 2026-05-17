//! Self-evolution engine — learns from interactions to create and improve skills.
//!
//! Inspired by Hermes Agent's learning loop:
//! 1. After complex tasks, extract reusable patterns as skills
//! 2. Track skill usage and outcomes for refinement
//! 3. Periodically self-evaluate and improve skills
//! 4. Export trajectory data for future model fine-tuning (RLHF)

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use tokio::sync::Mutex;

// ── Session Summary (for session-close hook) ──

/// Structured summary of a completed session, extracted on session close.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionSummary {
    pub session_id: String,
    pub timestamp: String,
    pub cwd: String,
    pub model: String,
    pub duration_secs: u64,
    /// Number of user→assistant turns
    pub turn_count: usize,
    /// All user messages (truncated to 200 chars each)
    pub user_topics: Vec<String>,
    /// Tool usage frequency: (tool_name, count)
    pub tool_usage: Vec<(String, u32)>,
    /// Tools that returned errors: (tool_name, error_preview)
    pub errors: Vec<(String, String)>,
    /// Total token usage
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_cache_read: u64,
    pub total_cost_usd: f64,
    /// Skills that were loaded/used during this session
    pub skills_used: Vec<String>,
}

// ── Configuration ──

const EVOLUTION_DIR: &str = "evolution";
const SKILL_CREATION_THRESHOLD: usize = 3; // min tool calls to consider a task "complex"
const SELF_EVAL_INTERVAL: usize = 15;      // evaluate every N completed tasks
const TRAJECTORY_FILE: &str = "trajectories.jsonl";
const SKILL_STATS_FILE: &str = "skill_stats.json";
const SESSION_SUMMARIES_FILE: &str = "session_summaries.jsonl";
const PENDING_REVIEW_FILE: &str = "pending_review.json";

// ── Data structures ──

/// A recorded interaction trajectory for RLHF training data.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Trajectory {
    pub id: String,
    pub timestamp: String,
    pub cwd: String,
    pub user_prompt: String,
    pub assistant_actions: Vec<TrajectoryAction>,
    pub outcome: TrajectoryOutcome,
    pub tool_count: usize,
    pub duration_ms: u64,
    /// User signal: was this interaction successful? None = not rated.
    pub user_rating: Option<TrajectoryRating>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TrajectoryAction {
    pub tool_name: String,
    pub input_summary: String,
    pub output_summary: String,
    pub is_error: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum TrajectoryOutcome {
    /// Task completed normally (end_turn)
    Completed { final_text_preview: String },
    /// Task hit max turns
    MaxTurns,
    /// Task was aborted by user
    Aborted,
    /// Task errored
    Error { code: String, message: String },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum TrajectoryRating {
    Good,
    Bad,
    Neutral,
}

/// Statistics for a single skill's usage and effectiveness.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SkillStats {
    pub skill_name: String,
    pub times_loaded: u32,
    pub times_relevant: u32,
    pub last_used: Option<String>,
    pub version: u32,
}

/// Candidate skill extracted from a successful interaction.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SkillCandidate {
    pub name: String,
    pub description: String,
    pub trigger_pattern: String,
    pub procedure: String,
    pub source_trajectory_id: String,
    pub created_at: String,
}

// ── Evolution Engine ──

pub struct EvolutionEngine {
    base_dir: Mutex<PathBuf>,
    task_count: Mutex<usize>,
}

impl EvolutionEngine {
    /// Create a new evolution engine.
    /// Uses global ~/.baoclaw/evolution/ for personal cross-project learning.
    pub fn new(_cwd: &Path) -> Self {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        let base_dir = PathBuf::from(home).join(".baoclaw").join(EVOLUTION_DIR);
        Self {
            base_dir: Mutex::new(base_dir),
            task_count: Mutex::new(0),
        }
    }

    /// Switch project — evolution data stays global, only resets task counter.
    pub async fn switch_project(&self, _cwd: &Path) {
        let mut count = self.task_count.lock().await;
        *count = 0;
    }

    /// Record a completed interaction as a trajectory.
    /// Called after each query loop completes.
    pub async fn record_trajectory(&self, trajectory: Trajectory) {
        let dir = self.base_dir.lock().await;
        let _ = std::fs::create_dir_all(&*dir);
        let traj_path = dir.join(TRAJECTORY_FILE);

        if let Ok(line) = serde_json::to_string(&trajectory) {
            use std::io::Write;
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&traj_path)
            {
                let _ = writeln!(f, "{}", line);
            }
        }

        // Increment task count
        let mut count = self.task_count.lock().await;
        *count += 1;

        // Check if we should trigger skill creation
        if trajectory.tool_count >= SKILL_CREATION_THRESHOLD {
            if let TrajectoryOutcome::Completed { .. } = &trajectory.outcome {
                let candidate = self.extract_skill_candidate(&trajectory);
                self.save_skill_candidate(&*dir, &candidate).await;
                eprintln!("Evolution: skill candidate '{}' extracted from trajectory {}",
                    candidate.name, trajectory.id);
            }
        }

        // Check if we should trigger self-evaluation
        if *count % SELF_EVAL_INTERVAL == 0 && *count > 0 {
            eprintln!("Evolution: self-evaluation triggered at task count {}", *count);
            // Self-evaluation is done asynchronously by the LLM in the next interaction
            // We write a nudge file that gets picked up by the system prompt builder
            let nudge_path = dir.join("pending_eval.json");
            let nudge = serde_json::json!({
                "type": "self_evaluation",
                "task_count": *count,
                "timestamp": chrono::Utc::now().to_rfc3339(),
            });
            let _ = std::fs::write(&nudge_path, serde_json::to_string_pretty(&nudge).unwrap_or_default());
        }
    }

    /// Extract a skill candidate from a successful trajectory.
    fn extract_skill_candidate(&self, trajectory: &Trajectory) -> SkillCandidate {
        // Build a procedure description from the tool actions
        let steps: Vec<String> = trajectory.assistant_actions.iter()
            .filter(|a| !a.is_error)
            .enumerate()
            .map(|(i, a)| format!("{}. Use `{}`: {}", i + 1, a.tool_name, a.input_summary))
            .collect();

        let procedure = steps.join("\n");

        // Derive a name from the user prompt (first 50 chars, slugified)
        let name_raw = trajectory.user_prompt.chars().take(50).collect::<String>();
        let name = slugify(&name_raw);

        SkillCandidate {
            name,
            description: format!("Auto-generated from: {}", 
                trajectory.user_prompt.chars().take(100).collect::<String>()),
            trigger_pattern: trajectory.user_prompt.chars().take(200).collect(),
            procedure,
            source_trajectory_id: trajectory.id.clone(),
            created_at: chrono::Utc::now().to_rfc3339(),
        }
    }

    /// Save a skill candidate to the candidates directory for review/promotion.
    async fn save_skill_candidate(&self, dir: &Path, candidate: &SkillCandidate) {
        let candidates_dir = dir.join("candidates");
        let _ = std::fs::create_dir_all(&candidates_dir);

        let filename = format!("{}.json", candidate.name);
        let path = candidates_dir.join(&filename);

        if let Ok(json) = serde_json::to_string_pretty(candidate) {
            let _ = std::fs::write(&path, json);
        }
    }

    /// Promote a skill candidate to an actual skill file.
    /// Skills go to ~/.baoclaw/skills/ (personal, cross-project) by default.
    pub async fn promote_skill(&self, _cwd: &Path, candidate_name: &str, 
                                skill_content: &str) -> Result<String, String> {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        let skills_dir = PathBuf::from(home).join(".baoclaw").join("skills");
        let _ = std::fs::create_dir_all(&skills_dir);

        let skill_path = skills_dir.join(format!("{}.md", candidate_name));
        std::fs::write(&skill_path, skill_content)
            .map_err(|e| format!("Failed to write skill: {}", e))?;

        // Remove the candidate file
        let dir = self.base_dir.lock().await;
        let candidate_path = dir.join("candidates").join(format!("{}.json", candidate_name));
        let _ = std::fs::remove_file(&candidate_path);

        eprintln!("Evolution: promoted skill '{}' to {}", candidate_name, skill_path.display());
        Ok(skill_path.to_string_lossy().to_string())
    }

    /// Record a user rating for the most recent trajectory.
    pub async fn rate_last_trajectory(&self, rating: TrajectoryRating) {
        let dir = self.base_dir.lock().await;
        let traj_path = dir.join(TRAJECTORY_FILE);

        // Read all trajectories, update the last one, rewrite
        if let Ok(content) = std::fs::read_to_string(&traj_path) {
            let mut lines: Vec<String> = content.lines()
                .filter(|l| !l.trim().is_empty())
                .map(|l| l.to_string())
                .collect();

            if let Some(last) = lines.last_mut() {
                if let Ok(mut traj) = serde_json::from_str::<Trajectory>(last) {
                    traj.user_rating = Some(rating);
                    if let Ok(updated) = serde_json::to_string(&traj) {
                        *last = updated;
                        let _ = std::fs::write(&traj_path, lines.join("\n") + "\n");
                    }
                }
            }
        }
    }

    /// List pending skill candidates.
    pub async fn list_candidates(&self) -> Vec<SkillCandidate> {
        let dir = self.base_dir.lock().await;
        let candidates_dir = dir.join("candidates");
        let mut candidates = Vec::new();

        if let Ok(entries) = std::fs::read_dir(&candidates_dir) {
            for entry in entries.flatten() {
                if entry.path().extension().map_or(false, |e| e == "json") {
                    if let Ok(content) = std::fs::read_to_string(entry.path()) {
                        if let Ok(candidate) = serde_json::from_str::<SkillCandidate>(&content) {
                            candidates.push(candidate);
                        }
                    }
                }
            }
        }

        candidates
    }

    /// Check if there's a pending self-evaluation nudge.
    pub async fn check_pending_eval(&self) -> Option<Value> {
        let dir = self.base_dir.lock().await;
        let nudge_path = dir.join("pending_eval.json");
        if nudge_path.exists() {
            let content = std::fs::read_to_string(&nudge_path).ok()?;
            let _ = std::fs::remove_file(&nudge_path); // consume the nudge
            serde_json::from_str(&content).ok()
        } else {
            None
        }
    }

    /// Build a system prompt fragment for the evolution system.
    /// Includes pending evaluations, session reviews, and skill candidates.
    pub async fn build_prompt_fragment(&self, _cwd: &Path) -> Option<String> {
        let mut parts = Vec::new();

        // Check for pending session review (from previous session's close hook)
        let dir = self.base_dir.lock().await;
        let review_path = dir.join(PENDING_REVIEW_FILE);
        if review_path.exists() {
            if let Ok(content) = std::fs::read_to_string(&review_path) {
                if let Ok(review) = serde_json::from_str::<Value>(&content) {
                    // Consume the review file
                    let _ = std::fs::remove_file(&review_path);

                    let session_id = review.get("session_id").and_then(|v| v.as_str()).unwrap_or("unknown");
                    let turn_count = review.get("turn_count").and_then(|v| v.as_u64()).unwrap_or(0);
                    let topics = review.get("user_topics")
                        .and_then(|v| v.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|v| v.as_str())
                                .take(10)
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    let tools = review.get("tools_used")
                        .and_then(|v| v.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|v| v.as_str())
                                .take(10)
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    let errors_count = review.get("errors_count").and_then(|v| v.as_u64()).unwrap_or(0);
                    let skills = review.get("skills_used")
                        .and_then(|v| v.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|v| v.as_str())
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();

                    let topics_str = if topics.is_empty() {
                        "  (none)".to_string()
                    } else {
                        topics.iter()
                            .enumerate()
                            .map(|(i, t)| format!("  {}. {}{}", i + 1,
                                t.chars().take(100).collect::<String>(),
                                if t.len() > 100 { "..." } else { "" }))
                            .collect::<Vec<_>>()
                            .join("\n")
                    };

                    let tools_str = if tools.is_empty() {
                        "  (none)".to_string()
                    } else {
                        tools.iter().map(|t| format!("  - {}", t)).collect::<Vec<_>>().join("\n")
                    };

                    parts.push(format!(
                        "# 🔁 Last Session Review (Auto-Generated)\n\
                        The previous session `{}` had {} turns. Here's what happened:\n\
                        \n\
                        ## User Topics:\n{}\n\
                        \n\
                        ## Tools Used:\n{}\n\
                        \n\
                        ## Errors: {}\n\
                        \n\
                        ## Skills Loaded: {}\n\
                        \n\
                        **Self-improvement nudge**: Reflect on the above. Ask yourself:\n\
                        - Were there repetitive patterns that should become a skill?\n\
                        - Did any errors reveal a gap in your knowledge or approach?\n\
                        - Should any preferences or decisions be saved to long-term memory?\n\
                        - Was there a workflow that could be streamlined?\n\
                        \n\
                        If yes, use the `Evolve` tool to create/improve skills, or `MemoryTool` to save insights.\n",
                        session_id,
                        turn_count,
                        topics_str,
                        tools_str,
                        errors_count,
                        if skills.is_empty() { "none".to_string() } else { skills.join(", ") },
                    ));
                }
            }
        }
        drop(dir); // release lock before calling other methods

        // Check for pending self-evaluation
        if let Some(eval) = self.check_pending_eval().await {
            let task_count = eval.get("task_count").and_then(|v| v.as_u64()).unwrap_or(0);
            parts.push(format!(
                "# Self-Evaluation Nudge\n\n\
                 You have completed {} tasks since the last evaluation. \
                 Take a moment to reflect:\n\
                 - What patterns have you noticed in the user's requests?\n\
                 - Are there repetitive workflows that could become skills?\n\
                 - Which of your approaches worked well vs poorly?\n\
                 Use the `evolve` tool to create or improve skills based on your observations.\n",
                task_count
            ));
        }

        // List pending skill candidates
        let candidates = self.list_candidates().await;
        if !candidates.is_empty() {
            parts.push("# Pending Skill Candidates\n\nThe following skill candidates were auto-extracted from successful interactions. Consider promoting the useful ones:\n".to_string());
            for c in &candidates {
                parts.push(format!(
                    "- **{}**: {}\n  Trigger: {}\n",
                    c.name,
                    c.description,
                    c.trigger_pattern.chars().take(80).collect::<String>()
                ));
            }
        }

        if parts.is_empty() {
            None
        } else {
            Some(parts.join("\n"))
        }
    }

    /// ── Session-close hook ──
    ///
    /// Called when the last client disconnects from a shared session.
    /// Extracts a structured summary from the session transcript and writes it
    /// to `session_summaries.jsonl`.  Also generates a `pending_review.json` that
    /// the *next* session's system prompt will pick up, guiding the LLM to
    /// reflect on what it learned.
    ///
    /// This is pure Rust — no LLM call, fast and reliable.
    pub async fn on_session_close(
        &self,
        session_id: &str,
        cwd: &str,
        model: &str,
        messages: &[crate::models::message::Message],
        total_usage: &crate::models::message::Usage,
        total_cost_usd: f64,
        session_duration_secs: u64,
    ) {
        use crate::models::message::{MessageContent, ContentBlock};

        let mut user_topics: Vec<String> = Vec::new();
        let mut tool_counts: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
        let mut errors: Vec<(String, String)> = Vec::new();
        let mut skills_used: Vec<String> = Vec::new();
        let mut turn_count: usize = 0;

        for msg in messages {
            match &msg.content {
                MessageContent::User { message, tool_use_result, .. } => {
                    // Extract user text topics
                    if tool_use_result.is_none() {
                        let text = extract_text_from_value(&message.content);
                        if !text.is_empty() {
                            let truncated: String = text.chars().take(200).collect();
                            user_topics.push(truncated);
                            turn_count += 1;
                        }
                    }
                    // Extract tool-result errors
                    if let Some(tr) = tool_use_result {
                        if tr.is_error {
                            let output_str = match &tr.output {
                                Value::String(s) => s.clone(),
                                other => serde_json::to_string(other).unwrap_or_default(),
                            };
                            let preview: String = output_str.chars().take(150).collect();
                            errors.push(("tool_result".to_string(), preview));
                        }
                    }
                }
                MessageContent::Assistant { message, .. } => {
                    for block in &message.content {
                        match block {
                            ContentBlock::ToolUse { name, input, .. } => {
                                *tool_counts.entry(name.clone()).or_insert(0) += 1;

                                // Detect skill loading (Skill tool calls)
                                if name == "Skill" {
                                    if let Some(s) = input.get("skill").and_then(|v| v.as_str()) {
                                        if s != "__list__" && !skills_used.contains(&s.to_string()) {
                                            skills_used.push(s.to_string());
                                        }
                                    }
                                }
                            }
                            ContentBlock::Text { .. } => {}
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }

        // Sort tool usage by count descending
        let mut tool_usage: Vec<(String, u32)> = tool_counts.into_iter().collect();
        tool_usage.sort_by(|a, b| b.1.cmp(&a.1));

        // Limit errors to 10
        errors.truncate(10);

        let summary = SessionSummary {
            session_id: session_id.to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            cwd: cwd.to_string(),
            model: model.to_string(),
            duration_secs: session_duration_secs,
            turn_count,
            user_topics,
            tool_usage,
            errors,
            total_input_tokens: total_usage.input_tokens,
            total_output_tokens: total_usage.output_tokens,
            total_cache_read: total_usage.cache_read_input_tokens.unwrap_or(0),
            total_cost_usd,
            skills_used,
        };

        // ── Persist to session_summaries.jsonl ──
        let dir = self.base_dir.lock().await;
        let _ = std::fs::create_dir_all(&*dir);
        let summaries_path = dir.join(SESSION_SUMMARIES_FILE);

        if let Ok(line) = serde_json::to_string(&summary) {
            use std::io::Write;
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&summaries_path)
            {
                let _ = writeln!(f, "{}", line);
            }
        }

        // ── Generate pending_review.json for next session ──
        // Only if the session had enough content to be worth reviewing
        if summary.turn_count >= 2 {
            let review = serde_json::json!({
                "type": "session_review",
                "session_id": summary.session_id,
                "timestamp": summary.timestamp,
                "cwd": summary.cwd,
                "turn_count": summary.turn_count,
                "duration_secs": summary.duration_secs,
                "total_cost_usd": summary.total_cost_usd,
                "tools_used": summary.tool_usage.iter()
                    .map(|(name, count)| format!("{} ({}×)", name, count))
                    .collect::<Vec<_>>(),
                "user_topics": summary.user_topics,
                "errors_count": summary.errors.len(),
                "skills_used": summary.skills_used,
            });
            let review_path = dir.join(PENDING_REVIEW_FILE);
            if let Ok(json) = serde_json::to_string_pretty(&review) {
                let _ = std::fs::write(&review_path, json);
            }
        }

        eprintln!(
            "Evolution: session-close hook for '{}' — {} turns, {} tools, {} errors, ${:.4} cost",
            session_id, summary.turn_count, summary.tool_usage.len(),
            summary.errors.len(), summary.total_cost_usd
        );
    }

    /// Export trajectories in a format suitable for RLHF/DPO fine-tuning.
    /// Returns pairs of (prompt, chosen_response, rejected_response) where available.
    pub async fn export_training_data(&self) -> Vec<Value> {
        let dir = self.base_dir.lock().await;
        let traj_path = dir.join(TRAJECTORY_FILE);
        let mut training_pairs = Vec::new();

        let content = match std::fs::read_to_string(&traj_path) {
            Ok(c) => c,
            Err(_) => return training_pairs,
        };

        let trajectories: Vec<Trajectory> = content.lines()
            .filter(|l| !l.trim().is_empty())
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect();

        // Group by similar prompts and create preference pairs
        // Good-rated completions are "chosen", bad-rated are "rejected"
        for traj in &trajectories {
            let actions_text: String = traj.assistant_actions.iter()
                .map(|a| format!("[{}] {}", a.tool_name, a.input_summary))
                .collect::<Vec<_>>()
                .join("\n");

            let outcome_text = match &traj.outcome {
                TrajectoryOutcome::Completed { final_text_preview } => final_text_preview.clone(),
                TrajectoryOutcome::MaxTurns => "[max turns reached]".to_string(),
                TrajectoryOutcome::Aborted => "[aborted by user]".to_string(),
                TrajectoryOutcome::Error { message, .. } => format!("[error: {}]", message),
            };

            let response = format!("{}\n\n{}", actions_text, outcome_text);

            let rating_label = match &traj.user_rating {
                Some(TrajectoryRating::Good) => "chosen",
                Some(TrajectoryRating::Bad) => "rejected",
                _ => "neutral",
            };

            training_pairs.push(serde_json::json!({
                "prompt": traj.user_prompt,
                "response": response,
                "rating": rating_label,
                "tool_count": traj.tool_count,
                "duration_ms": traj.duration_ms,
                "cwd": traj.cwd,
                "timestamp": traj.timestamp,
            }));
        }

        training_pairs
    }
}

/// Extract plain text from a serde_json::Value that could be a string or
/// an array of content blocks (Claude API format).
fn extract_text_from_value(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Array(blocks) => {
            let mut texts = Vec::new();
            for block in blocks {
                if let Some(text) = block.get("text").and_then(|v| v.as_str()) {
                    texts.push(text.to_string());
                }
            }
            texts.join(" ")
        }
        _ => String::new(),
    }
}

/// Simple slugify: lowercase, replace non-alphanumeric with hyphens, trim.
fn slugify(s: &str) -> String {
    let slug: String = s.chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
        .collect();
    let slug = slug.trim_matches('-').to_lowercase();
    // Collapse multiple hyphens
    let mut result = String::new();
    let mut prev_hyphen = false;
    for c in slug.chars() {
        if c == '-' {
            if !prev_hyphen { result.push(c); }
            prev_hyphen = true;
        } else {
            result.push(c);
            prev_hyphen = false;
        }
    }
    if result.len() > 60 { result.truncate(60); }
    if result.is_empty() { "auto-skill".to_string() } else { result }
}
