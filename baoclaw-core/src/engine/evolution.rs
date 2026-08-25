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
    pub times_succeeded: u32,
    pub times_failed: u32,
    pub last_used: Option<String>,
    pub version: u32,
    /// Average user rating (0.0-1.0) across sessions where this skill was used.
    pub avg_rating: f64,
    /// Whether this skill has been retired (auto-disabled due to poor performance).
    pub retired: bool,
    /// Reason for retirement, if retired.
    pub retired_reason: Option<String>,
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

impl SkillStats {
    fn new(name: &str) -> Self {
        Self {
            skill_name: name.to_string(),
            times_loaded: 0,
            times_relevant: 0,
            times_succeeded: 0,
            times_failed: 0,
            last_used: None,
            version: 1,
            avg_rating: 0.0,
            retired: false,
            retired_reason: None,
        }
    }
}

// ── Phase 2 #8: Skill Self-Improvement Data Structures ──

/// Grade assigned to a skill during evaluation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum SkillGrade {
    Excellent,
    Good,
    NeedsImprovement,
    Poor,
    Critical,
    InsufficientData,
}

/// Suggested action based on skill evaluation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum SuggestedAction {
    None,
    MinorTweak,
    Improve,
    MajorRevision,
    Retire,
}

/// Result of evaluating a skill's effectiveness.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SkillEvaluation {
    pub skill_name: String,
    pub score: f64,
    pub grade: SkillGrade,
    pub diagnostics: Vec<String>,
    pub suggested_action: SuggestedAction,
}

/// A single improvement suggestion for a skill.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ImprovementSuggestion {
    pub priority: u32,
    pub category: String,
    pub description: String,
}

/// Severity of a validation issue.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Severity {
    Error,
    Warning,
}

/// A single issue found during skill validation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ValidationIssue {
    pub severity: Severity,
    pub message: String,
}

/// Result of validating a skill's integrity.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ValidationResult {
    pub skill_name: String,
    pub valid: bool,
    pub issues: Vec<ValidationIssue>,
}

/// Summary report from running a full improvement cycle.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ImprovementCycleReport {
    pub skills_evaluated: usize,
    pub skills_improved: usize,
    pub skills_retired: usize,
    pub actions: Vec<String>,
}

// ── Evolution Engine ──

pub struct EvolutionEngine {
    base_dir: Mutex<PathBuf>,
    task_count: Mutex<usize>,
    skills_dir: PathBuf,
    skill_stats: tokio::sync::Mutex<std::collections::HashMap<String, SkillStats>>,
    trajectories: tokio::sync::Mutex<Vec<Trajectory>>,
}

impl EvolutionEngine {
    /// Create a new evolution engine.
    /// Uses global ~/.baoclaw/evolution/ for personal cross-project learning.
    pub fn new(_cwd: &Path) -> Self {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        let base_dir = PathBuf::from(&home).join(".baoclaw").join(EVOLUTION_DIR);
        let skills_dir = PathBuf::from(&home).join(".baoclaw").join("skills");
        let _ = std::fs::create_dir_all(&skills_dir);
        Self {
            base_dir: Mutex::new(base_dir),
            task_count: Mutex::new(0),
            skills_dir,
            skill_stats: tokio::sync::Mutex::new(std::collections::HashMap::new()),
            trajectories: tokio::sync::Mutex::new(Vec::new()),
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
                self.save_skill_candidate(&dir, &candidate).await;
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
            if let Err(e) = std::fs::write(&nudge_path, serde_json::to_string_pretty(&nudge).unwrap_or_default()) {
                eprintln!("[evolution] WARNING: could not write nudge file {}: {}", nudge_path.display(), e);
            }
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
        if let Err(e) = std::fs::create_dir_all(&candidates_dir) {
            eprintln!("[evolution] WARNING: could not create candidates dir {}: {}", candidates_dir.display(), e);
        }

        let filename = format!("{}.json", candidate.name);
        let path = candidates_dir.join(&filename);

        if let Ok(json) = serde_json::to_string_pretty(candidate) {
            if let Err(e) = std::fs::write(&path, json) {
                eprintln!("[evolution] WARNING: could not write candidate {}: {}", path.display(), e);
            }
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
                        if let Err(e) = std::fs::write(&traj_path, lines.join("\n") + "\n") {
                            eprintln!("[evolution] WARNING: could not update trajectory {}: {}", traj_path.display(), e);
                        }
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
                if entry.path().extension().is_some_and(|e| e == "json") {
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
    #[allow(clippy::too_many_arguments)]
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
        tool_usage.sort_by_key(|a| std::cmp::Reverse(a.1));

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

    // -----------------------------------------------------------------------
    // Phase 2 #8: Skill Self-Improvement Loop (5-stage cycle)
    //   1. Collect  — record_trajectory (already exists)
    //   2. Evaluate — evaluate_skill effectiveness
    //   3. Improve  — generate improvement suggestions
    //   4. Validate — check skill integrity
    //   5. Retire   — auto-disable persistently poor skills
    // -----------------------------------------------------------------------

    /// Stage 2: Evaluate a skill's effectiveness based on accumulated stats.
    ///
    /// Returns a [`SkillEvaluation`] with score (0.0–1.0) and diagnostics.
    pub async fn evaluate_skill(&self, skill_name: &str) -> SkillEvaluation {
        let stats = self.get_or_create_stats(skill_name).await;

        // Skip evaluation if too few data points
        if stats.times_loaded < 3 {
            return SkillEvaluation {
                skill_name: skill_name.to_string(),
                score: 1.0, // neutral — not enough data
                grade: SkillGrade::InsufficientData,
                diagnostics: vec!["Not enough usage data for evaluation (need ≥3 loads)".into()],
                suggested_action: SuggestedAction::None,
            };
        }

        let total_invocations = stats.times_relevant;
        let total_outcomes = stats.times_succeeded + stats.times_failed;
        let mut diagnostics: Vec<String> = Vec::new();
        let mut score = 1.0_f64;

        // Factor 1: Relevance rate (loaded vs actually relevant)
        if total_invocations > 0 {
            let relevance_rate = stats.times_relevant as f64 / stats.times_loaded as f64;
            if relevance_rate < 0.3 {
                score *= 0.6;
                diagnostics.push(format!(
                    "Low relevance rate: {:.0}% (loaded {} times, relevant {} times)",
                    relevance_rate * 100.0,
                    stats.times_loaded,
                    stats.times_relevant,
                ));
            } else if relevance_rate < 0.5 {
                score *= 0.8;
                diagnostics.push(format!(
                    "Moderate relevance rate: {:.0}%",
                    relevance_rate * 100.0,
                ));
            }
        }

        // Factor 2: Success rate (when invoked, did it help?)
        if total_outcomes > 0 {
            let success_rate = stats.times_succeeded as f64 / total_outcomes as f64;
            if success_rate < 0.4 {
                score *= 0.5;
                diagnostics.push(format!(
                    "Low success rate: {:.0}% ({} success, {} fail)",
                    success_rate * 100.0,
                    stats.times_succeeded,
                    stats.times_failed,
                ));
            } else if success_rate < 0.7 {
                score *= 0.85;
                diagnostics.push(format!(
                    "Moderate success rate: {:.0}%",
                    success_rate * 100.0,
                ));
            }
        }

        // Factor 3: User rating (if available)
        if stats.avg_rating > 0.0 {
            score *= stats.avg_rating;
            if stats.avg_rating < 0.4 {
                diagnostics.push(format!("Low user rating: {:.2}", stats.avg_rating));
            }
        }

        // Factor 4: Version staleness (old skills may be outdated)
        if stats.version < 2 && stats.times_loaded > 10 {
            score *= 0.95;
            diagnostics.push("Skill has not been improved despite heavy usage".into());
        }

        // Clamp score
        score = score.clamp(0.0, 1.0);

        // Determine grade and suggested action
        let (grade, suggested_action) = if score >= 0.8 {
            (SkillGrade::Excellent, SuggestedAction::None)
        } else if score >= 0.6 {
            (SkillGrade::Good, SuggestedAction::MinorTweak)
        } else if score >= 0.4 {
            (SkillGrade::NeedsImprovement, SuggestedAction::Improve)
        } else if score >= 0.2 {
            (SkillGrade::Poor, SuggestedAction::MajorRevision)
        } else {
            (SkillGrade::Critical, SuggestedAction::Retire)
        };

        if diagnostics.is_empty() {
            diagnostics.push("Skill performing within acceptable parameters".into());
        }

        SkillEvaluation {
            skill_name: skill_name.to_string(),
            score,
            grade,
            diagnostics,
            suggested_action,
        }
    }

    /// Stage 3: Generate improvement suggestions for a skill based on
    /// its evaluation and recent failure trajectories.
    pub async fn suggest_improvements(&self, skill_name: &str) -> Vec<ImprovementSuggestion> {
        let evaluation = self.evaluate_skill(skill_name).await;
        let mut suggestions = Vec::new();

        if matches!(evaluation.suggested_action, SuggestedAction::None | SuggestedAction::MinorTweak) {
            return suggestions; // nothing to improve
        }

        // Analyze failure patterns from trajectories
        let trajectories = self.trajectories.lock().await;
        let mut failure_tools: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
        let mut failure_keywords: std::collections::HashMap<String, u32> = std::collections::HashMap::new();

        for traj in trajectories.iter() {
            // Match by user_prompt containing skill_name (Trajectory has no skill_name field)
            if traj.user_prompt.contains(skill_name)
                && matches!(traj.user_rating, Some(TrajectoryRating::Bad))
            {
                // Track which tools were used in failed trajectories
                for action in &traj.assistant_actions {
                    *failure_tools.entry(action.tool_name.clone()).or_insert(0) += 1;
                }
                // Extract keywords from user prompt (simple word split)
                for word in traj.user_prompt.split_whitespace() {
                    let w = word.to_lowercase();
                    if w.len() > 4 {
                        *failure_keywords.entry(w).or_insert(0) += 1;
                    }
                }
            }
        }
        drop(trajectories);

        // Suggestion 1: Based on grade
        match evaluation.grade {
            SkillGrade::NeedsImprovement => {
                suggestions.push(ImprovementSuggestion {
                    priority: 1,
                    category: "performance".into(),
                    description: format!(
                        "Skill '{}' has moderate effectiveness (score {:.2}). Review trigger conditions to reduce false positives.",
                        skill_name, evaluation.score
                    ),
                });
            }
            SkillGrade::Poor => {
                suggestions.push(ImprovementSuggestion {
                    priority: 2,
                    category: "relevance".into(),
                    description: format!(
                        "Skill '{}' is underperforming (score {:.2}). Consider narrowing scope or improving instructions.",
                        skill_name, evaluation.score
                    ),
                });
            }
            SkillGrade::Critical => {
                suggestions.push(ImprovementSuggestion {
                    priority: 3,
                    category: "retirement".into(),
                    description: format!(
                        "Skill '{}' is critically ineffective (score {:.2}). Recommend retirement or complete rewrite.",
                        skill_name, evaluation.score
                    ),
                });
            }
            _ => {}
        }

        // Suggestion 2: Based on failure tool patterns
        if let Some((most_failed_tool, count)) = failure_tools.iter().max_by_key(|(_, c)| *c) {
            if *count >= 2 {
                suggestions.push(ImprovementSuggestion {
                    priority: 2,
                    category: "tool_usage".into(),
                    description: format!(
                        "Tool '{}' appears in {} failed trajectories. Consider adjusting tool invocation strategy.",
                        most_failed_tool, count
                    ),
                });
            }
        }

        // Suggestion 3: Based on failure keywords
        let mut sorted_keywords: Vec<_> = failure_keywords.iter().collect();
        sorted_keywords.sort_by(|a, b| b.1.cmp(a.1));
        if sorted_keywords.len() >= 2 {
            let top_kw: Vec<&str> = sorted_keywords.iter().take(3).map(|(k, _)| k.as_str()).collect();
            suggestions.push(ImprovementSuggestion {
                priority: 1,
                category: "scope".into(),
                description: format!(
                    "Common keywords in failures: {}. May indicate scope mismatch.",
                    top_kw.join(", ")
                ),
            });
        }

        // Suggestion 4: Based on diagnostics from evaluation
        for diag in &evaluation.diagnostics {
            if diag.contains("relevance") {
                suggestions.push(ImprovementSuggestion {
                    priority: 2,
                    category: "trigger".into(),
                    description: "Improve trigger condition specificity to reduce irrelevant activations".into(),
                });
            }
            if diag.contains("success rate") {
                suggestions.push(ImprovementSuggestion {
                    priority: 2,
                    category: "instructions".into(),
                    description: "Strengthen step-by-step instructions to improve execution success rate".into(),
                });
            }
        }

        suggestions.sort_by_key(|a| std::cmp::Reverse(a.priority));
        suggestions
    }

    /// Stage 4: Validate a skill's integrity (syntax, completeness, structure).
    ///
    /// Returns a list of issues found (empty = valid).
    pub async fn validate_skill(&self, skill_name: &str) -> ValidationResult {
        let mut issues: Vec<ValidationIssue> = Vec::new();

        // Load skill content
        let skill_path = self.skills_dir.join(format!("{}.md", skill_name));
        if !skill_path.exists() {
            return ValidationResult {
                skill_name: skill_name.to_string(),
                valid: false,
                issues: vec![ValidationIssue {
                    severity: Severity::Error,
                    message: format!("Skill file not found: {}", skill_path.display()),
                }],
            };
        }

        let content = match std::fs::read_to_string(&skill_path) {
            Ok(c) => c,
            Err(e) => {
                return ValidationResult {
                    skill_name: skill_name.to_string(),
                    valid: false,
                    issues: vec![ValidationIssue {
                        severity: Severity::Error,
                        message: format!("Cannot read skill file: {}", e),
                    }],
                };
            }
        };

        // Check 1: Non-empty
        if content.trim().is_empty() {
            issues.push(ValidationIssue {
                severity: Severity::Error,
                message: "Skill file is empty".into(),
            });
        }

        // Check 2: Has at least one section header
        let section_count = content.lines().filter(|l| l.starts_with("## ")).count();
        if section_count == 0 {
            issues.push(ValidationIssue {
                severity: Severity::Warning,
                message: "No section headers (## ) found — skill may lack structure".into(),
            });
        }

        // Check 3: Has trigger conditions
        let has_trigger = content.lines().any(|l| {
            let lower = l.to_lowercase();
            lower.contains("trigger") || lower.contains("when to use") || lower.contains("use when")
        });
        if !has_trigger {
            issues.push(ValidationIssue {
                severity: Severity::Warning,
                message: "No trigger conditions found — skill may activate incorrectly".into(),
            });
        }

        // Check 4: Has step-by-step instructions
        let has_steps = content.lines().any(|l| {
            l.starts_with("1.") || l.starts_with("- ") || l.starts_with("* ") || l.starts_with("Step")
        });
        if !has_steps {
            issues.push(ValidationIssue {
                severity: Severity::Warning,
                message: "No step-by-step instructions found".into(),
            });
        }

        // Check 5: Reasonable length (not too short, not too long)
        let line_count = content.lines().count();
        if line_count < 5 {
            issues.push(ValidationIssue {
                severity: Severity::Warning,
                message: format!("Skill is very short ({} lines) — may be incomplete", line_count),
            });
        } else if line_count > 500 {
            issues.push(ValidationIssue {
                severity: Severity::Warning,
                message: format!("Skill is very long ({} lines) — consider splitting", line_count),
            });
        }

        // Check 6: No broken markdown links
        for line in content.lines() {
            if line.contains("](") && !line.contains("](http") && !line.contains("](/") {
                // Relative link without path — might be broken
                if line.contains("](#") {
                    continue; // anchor links are fine
                }
            }
        }

        let valid = issues.iter().all(|i| !matches!(i.severity, Severity::Error));
        ValidationResult {
            skill_name: skill_name.to_string(),
            valid,
            issues,
        }
    }

    /// Stage 5: Retire a persistently poor skill.
    ///
    /// Marks the skill as retired (skipped during skill loading) and writes
    /// a retirement notice into the skill file. Can be un-retired later.
    pub async fn retire_skill(&self, skill_name: &str, reason: &str) -> Result<(), String> {
        // Update stats
        {
            let mut stats_lock = self.skill_stats.lock().await;
            if let Some(stats) = stats_lock.get_mut(skill_name) {
                stats.retired = true;
                stats.retired_reason = Some(reason.to_string());
            } else {
                let mut stats = SkillStats::new(skill_name);
                stats.retired = true;
                stats.retired_reason = Some(reason.to_string());
                stats_lock.insert(skill_name.to_string(), stats);
            }
        }

        // Persist retirement notice in skill file
        let skill_path = self.skills_dir.join(format!("{}.md", skill_name));
        if skill_path.exists() {
            if let Ok(content) = std::fs::read_to_string(&skill_path) {
                let notice = format!(
                    "\n\n---\n> ⚠️ **RETIRED** (auto-disabled on {})\n> Reason: {}\n> To re-enable: remove this notice and set `retired: false` in stats.\n",
                    chrono::Utc::now().format("%Y-%m-%d"),
                    reason,
                );
                let _ = std::fs::write(&skill_path, format!("{}{}", content, notice));
            }
        }

        // Save updated stats
        let _ = self.save_stats().await;

        Ok(())
    }

    /// Un-retire a previously retired skill.
    pub async fn unretire_skill(&self, skill_name: &str) -> Result<(), String> {
        {
            let mut stats_lock = self.skill_stats.lock().await;
            if let Some(stats) = stats_lock.get_mut(skill_name) {
                stats.retired = false;
                stats.retired_reason = None;
            }
        }
        let _ = self.save_stats().await;
        Ok(())
    }

    /// Run the full evaluation-improvement cycle for all skills.
    ///
    /// Called periodically (e.g., every 10 sessions) to auto-improve the skill set.
    /// Returns a summary of actions taken.
    pub async fn run_improvement_cycle(&self) -> ImprovementCycleReport {
        let mut report = ImprovementCycleReport {
            skills_evaluated: 0,
            skills_improved: 0,
            skills_retired: 0,
            actions: Vec::new(),
        };

        let skill_names: Vec<String> = {
            let stats = self.skill_stats.lock().await;
            stats.keys().cloned().collect()
        };

        for name in &skill_names {
            let evaluation = self.evaluate_skill(name).await;
            report.skills_evaluated += 1;

            match evaluation.suggested_action {
                SuggestedAction::Retire => {
                    let reason = format!(
                        "Auto-retired: score {:.2}, grade {:?}",
                        evaluation.score, evaluation.grade
                    );
                    let _ = self.retire_skill(name, &reason).await;
                    report.skills_retired += 1;
                    report.actions.push(format!("RETIRE: {} ({})", name, reason));
                }
                SuggestedAction::MajorRevision | SuggestedAction::Improve => {
                    report.skills_improved += 1;
                    let suggestions = self.suggest_improvements(name).await;
                    let sug_summary: Vec<String> = suggestions.iter()
                        .take(3)
                        .map(|s| format!("[{}] {}", s.category, s.description))
                        .collect();
                    report.actions.push(format!(
                        "IMPROVE: {} — {} suggestions: {}",
                        name,
                        suggestions.len(),
                        sug_summary.join("; "),
                    ));
                }
                SuggestedAction::MinorTweak => {
                    report.actions.push(format!(
                        "TWEAK: {} (score {:.2}, acceptable)",
                        name, evaluation.score,
                    ));
                }
                SuggestedAction::None => {
                    // Skill is healthy, no action needed
                }
            }
        }

        report
    }

    /// Helper: get or create stats for a skill.
    async fn get_or_create_stats(&self, skill_name: &str) -> SkillStats {
        let mut stats = self.skill_stats.lock().await;
        stats.entry(skill_name.to_string())
            .or_insert_with(|| SkillStats::new(skill_name))
            .clone()
    }

    /// Record a skill invocation outcome (success or failure).
    pub async fn record_skill_outcome(&self, skill_name: &str, success: bool) {
        let mut stats = self.skill_stats.lock().await;
        let entry = stats.entry(skill_name.to_string())
            .or_insert_with(|| SkillStats::new(skill_name));
        if success {
            entry.times_succeeded += 1;
        } else {
            entry.times_failed += 1;
        }
        entry.last_used = Some(chrono::Utc::now().to_rfc3339());
        drop(stats);
        let _ = self.save_stats().await;
    }

    /// Persist skill stats to disk.
    async fn save_stats(&self) -> Result<(), String> {
        let stats = self.skill_stats.lock().await;
        let json = serde_json::to_string_pretty(&*stats)
            .map_err(|e| format!("Failed to serialize stats: {}", e))?;
        let stats_path = self.skills_dir.join("skill_stats.json");
        std::fs::write(&stats_path, json)
            .map_err(|e| format!("Failed to write stats: {}", e))?;
        Ok(())
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
