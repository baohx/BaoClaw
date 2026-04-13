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

// ── Configuration ──

const EVOLUTION_DIR: &str = "evolution";
const SKILL_CREATION_THRESHOLD: usize = 3; // min tool calls to consider a task "complex"
const SELF_EVAL_INTERVAL: usize = 15;      // evaluate every N completed tasks
const TRAJECTORY_FILE: &str = "trajectories.jsonl";
const SKILL_STATS_FILE: &str = "skill_stats.json";

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
    /// Create a new evolution engine for a project directory.
    pub fn new(cwd: &Path) -> Self {
        let base_dir = cwd.join(".baoclaw").join(EVOLUTION_DIR);
        Self {
            base_dir: Mutex::new(base_dir),
            task_count: Mutex::new(0),
        }
    }

    /// Switch to a new project directory.
    pub async fn switch_project(&self, cwd: &Path) {
        let mut dir = self.base_dir.lock().await;
        *dir = cwd.join(".baoclaw").join(EVOLUTION_DIR);
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
    /// Called by the LLM during self-evaluation or by user command.
    pub async fn promote_skill(&self, cwd: &Path, candidate_name: &str, 
                                skill_content: &str) -> Result<String, String> {
        let skills_dir = cwd.join(".baoclaw").join("skills");
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
    /// Includes pending evaluations and skill candidates.
    pub async fn build_prompt_fragment(&self, cwd: &Path) -> Option<String> {
        let mut parts = Vec::new();

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
