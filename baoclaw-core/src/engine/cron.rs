//! Cron scheduler — runs periodic tasks inside the daemon.
//!
//! Jobs are stored in ~/.baoclaw/cron.json and persist across daemon restarts.
//! Each job runs in a fresh query session and results are broadcast to all
//! connected clients (CLI, Telegram, WhatsApp).

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex};

const CRON_FILE: &str = "cron.json";

// ── Data structures ──

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CronJob {
    pub id: String,
    pub name: String,
    pub prompt: String,
    /// Cron expression: "every 1h", "every 30m", "daily 09:00", "weekly mon 09:00"
    pub schedule: String,
    /// Which project cwd to run in (None = global)
    pub cwd: Option<String>,
    pub enabled: bool,
    pub created_at: String,
    pub last_run: Option<String>,
    pub last_result: Option<String>,
}

#[derive(Clone, Debug)]
pub struct CronResult {
    pub job_id: String,
    pub job_name: String,
    pub text: String,
    pub timestamp: String,
}

/// Parsed schedule for internal use.
#[derive(Debug)]
enum Schedule {
    /// Every N seconds
    Interval(u64),
    /// Daily at HH:MM (UTC)
    Daily { hour: u32, minute: u32 },
    /// Weekly on day at HH:MM (UTC), day 0=Mon..6=Sun
    Weekly { day: u32, hour: u32, minute: u32 },
}

// ── Cron Manager ──

pub struct CronManager {
    jobs: Mutex<Vec<CronJob>>,
    config_path: PathBuf,
    /// Broadcast channel for cron results → all clients
    result_tx: broadcast::Sender<CronResult>,
}

impl Default for CronManager {
    fn default() -> Self {
        Self::new()
    }
}

impl CronManager {
    pub fn new() -> Self {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        let config_path = PathBuf::from(&home).join(".baoclaw").join(CRON_FILE);
        let jobs = Self::load_jobs(&config_path);
        let (result_tx, _) = broadcast::channel(64);
        eprintln!(
            "Cron: loaded {} jobs from {}",
            jobs.len(),
            config_path.display()
        );
        Self {
            jobs: Mutex::new(jobs),
            config_path,
            result_tx,
        }
    }

    /// Subscribe to cron results (for clients to receive notifications).
    pub fn subscribe(&self) -> broadcast::Receiver<CronResult> {
        self.result_tx.subscribe()
    }

    /// Add a new cron job.
    pub async fn add_job(
        &self,
        name: String,
        prompt: String,
        schedule: String,
        cwd: Option<String>,
    ) -> Result<CronJob, String> {
        // Validate schedule
        parse_schedule(&schedule).map_err(|e| format!("Invalid schedule '{}': {}", schedule, e))?;

        let job = CronJob {
            id: uuid::Uuid::new_v4().to_string()[..8].to_string(),
            name,
            prompt,
            schedule,
            cwd,
            enabled: true,
            created_at: chrono::Utc::now().to_rfc3339(),
            last_run: None,
            last_result: None,
        };

        let mut jobs = self.jobs.lock().await;
        let mut persisted_jobs = jobs.clone();
        persisted_jobs.push(job.clone());
        self.save_jobs(&persisted_jobs)?;
        jobs.push(job.clone());
        Ok(job)
    }

    /// Remove a job by ID.
    pub async fn remove_job(&self, id: &str) -> bool {
        let mut jobs = self.jobs.lock().await;
        let before = jobs.len();
        jobs.retain(|j| j.id != id);
        if jobs.len() < before {
            if let Err(error) = self.save_jobs(&jobs) {
                eprintln!("Cron: failed to persist removed job: {error}");
            }
            true
        } else {
            false
        }
    }

    /// Enable/disable a job.
    pub async fn toggle_job(&self, id: &str) -> Option<bool> {
        let mut jobs = self.jobs.lock().await;
        let mut result = None;
        for job in jobs.iter_mut() {
            if job.id == id {
                job.enabled = !job.enabled;
                result = Some(job.enabled);
                break;
            }
        }
        if result.is_some() {
            if let Err(error) = self.save_jobs(&jobs) {
                eprintln!("Cron: failed to persist toggled job: {error}");
            }
        }
        result
    }

    /// List all jobs.
    pub async fn list_jobs(&self) -> Vec<CronJob> {
        self.jobs.lock().await.clone()
    }

    /// Update last_run and last_result for a job.
    async fn update_job_result(&self, id: &str, result: &str) {
        let mut jobs = self.jobs.lock().await;
        for job in jobs.iter_mut() {
            if job.id == id {
                job.last_run = Some(chrono::Utc::now().to_rfc3339());
                job.last_result = Some(result.chars().take(500).collect());
                break;
            }
        }
        if let Err(error) = self.save_jobs(&jobs) {
            eprintln!("Cron: failed to persist job result: {error}");
        }
    }

    fn load_jobs(path: &PathBuf) -> Vec<CronJob> {
        match std::fs::read_to_string(path) {
            Ok(content) => serde_json::from_str(&content).unwrap_or_else(|error| {
                eprintln!(
                    "Cron: cannot load {} because the state is malformed: {}. No jobs loaded.",
                    path.display(),
                    error
                );
                Vec::new()
            }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(error) => {
                eprintln!(
                    "Cron: cannot load {} because the state file could not be read: {}. No jobs loaded.",
                    path.display(),
                    error
                );
                Vec::new()
            }
        }
    }

    fn save_jobs(&self, jobs: &[CronJob]) -> Result<(), String> {
        let json = serde_json::to_string_pretty(jobs).map_err(|error| {
            format!("Cannot save cron jobs because serialization failed: {error}")
        })?;
        if let Some(parent) = self.config_path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                format!("Cannot save cron jobs because directory creation failed: {error}")
            })?;
        }
        atomic_write_json(&self.config_path, &json).map_err(|error| {
            format!("Cannot save cron jobs because the state file could not be written: {error}")
        })
    }

    /// Start the cron scheduler loop. Runs forever, checking jobs every 30 seconds.
    /// When a job fires, it calls the provided `run_fn` to execute the prompt
    /// and broadcasts the result to all subscribers.
    pub async fn start_scheduler(
        self: Arc<Self>,
        run_fn: Arc<
            dyn Fn(String, Option<String>) -> tokio::task::JoinHandle<String> + Send + Sync,
        >,
    ) {
        eprintln!("Cron: scheduler started");
        let mut last_check: std::collections::HashMap<String, std::time::Instant> =
            std::collections::HashMap::new();

        loop {
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;

            let jobs = self.jobs.lock().await.clone();
            let now = chrono::Utc::now();

            for job in &jobs {
                if !job.enabled {
                    continue;
                }

                let schedule = match parse_schedule(&job.schedule) {
                    Ok(s) => s,
                    Err(_) => continue,
                };

                let should_run = match &schedule {
                    Schedule::Interval(secs) => {
                        let last = last_check.get(&job.id).copied().unwrap_or_else(|| {
                            std::time::Instant::now() - std::time::Duration::from_secs(*secs)
                        });
                        last.elapsed().as_secs() >= *secs
                    }
                    Schedule::Daily { hour, minute } => {
                        let now_h = now.format("%H").to_string().parse::<u32>().unwrap_or(0);
                        let now_m = now.format("%M").to_string().parse::<u32>().unwrap_or(0);
                        let time_match = now_h == *hour && now_m == *minute;
                        let last = last_check.get(&job.id);
                        let not_recently = last.is_none_or(|l| l.elapsed().as_secs() > 120);
                        time_match && not_recently
                    }
                    Schedule::Weekly { day, hour, minute } => {
                        let now_dow = now.format("%u").to_string().parse::<u32>().unwrap_or(1) - 1; // 0=Mon
                        let now_h = now.format("%H").to_string().parse::<u32>().unwrap_or(0);
                        let now_m = now.format("%M").to_string().parse::<u32>().unwrap_or(0);
                        let time_match = now_dow == *day && now_h == *hour && now_m == *minute;
                        let last = last_check.get(&job.id);
                        let not_recently = last.is_none_or(|l| l.elapsed().as_secs() > 120);
                        time_match && not_recently
                    }
                };

                if should_run {
                    last_check.insert(job.id.clone(), std::time::Instant::now());
                    eprintln!("Cron: firing job '{}' ({})", job.name, job.id);

                    let job_clone = job.clone();
                    let self_clone = Arc::clone(&self);
                    let run_fn_clone = Arc::clone(&run_fn);

                    tokio::spawn(async move {
                        let handle = run_fn_clone(job_clone.prompt.clone(), job_clone.cwd.clone());
                        let result_text = handle
                            .await
                            .unwrap_or_else(|e| format!("Cron job error: {}", e));

                        // Update job stats
                        self_clone
                            .update_job_result(&job_clone.id, &result_text)
                            .await;

                        // Broadcast result to all clients
                        let cron_result = CronResult {
                            job_id: job_clone.id.clone(),
                            job_name: job_clone.name.clone(),
                            text: result_text,
                            timestamp: chrono::Utc::now().to_rfc3339(),
                        };
                        let _ = self_clone.result_tx.send(cron_result);
                    });
                }
            }
        }
    }
}

// ── Schedule parsing ──

fn parse_schedule(s: &str) -> Result<Schedule, String> {
    let s = s.trim().to_lowercase();

    // "every 30m", "every 1h", "every 2h30m", "every 60s"
    if let Some(rest) = s.strip_prefix("every ") {
        let secs = parse_duration(rest)?;
        if secs < 60 {
            return Err("Minimum interval is 60 seconds".to_string());
        }
        return Ok(Schedule::Interval(secs));
    }

    // "daily 09:00"
    if let Some(time) = s.strip_prefix("daily ") {
        let (h, m) = parse_time(time)?;
        return Ok(Schedule::Daily { hour: h, minute: m });
    }

    // "weekly mon 09:00"
    if let Some(stripped) = s.strip_prefix("weekly ") {
        let parts: Vec<&str> = stripped.split_whitespace().collect();
        if parts.len() != 2 {
            return Err("Expected: weekly <day> <HH:MM>".to_string());
        }
        let day = parse_day(parts[0])?;
        let (h, m) = parse_time(parts[1])?;
        return Ok(Schedule::Weekly {
            day,
            hour: h,
            minute: m,
        });
    }

    Err(format!(
        "Unknown schedule format: '{}'. Use: every <duration>, daily <HH:MM>, weekly <day> <HH:MM>",
        s
    ))
}

fn parse_duration(s: &str) -> Result<u64, String> {
    let mut total: u64 = 0;
    let mut num_buf = String::new();

    for c in s.chars() {
        if c.is_ascii_digit() {
            num_buf.push(c);
        } else {
            let n: u64 = num_buf
                .parse()
                .map_err(|_| format!("Invalid number in duration: {}", s))?;
            num_buf.clear();
            let multiplier = match c {
                's' => 1,
                'm' => 60,
                'h' => 3600,
                'd' => 86400,
                _ => return Err(format!("Unknown duration unit: {}", c)),
            };
            total = total
                .checked_add(
                    n.checked_mul(multiplier)
                        .ok_or_else(|| format!("Duration is too large: {s}"))?,
                )
                .ok_or_else(|| format!("Duration is too large: {s}"))?;
        }
    }

    // Handle bare number (assume minutes)
    if !num_buf.is_empty() {
        let n: u64 = num_buf
            .parse()
            .map_err(|_| format!("Invalid number: {}", s))?;
        total = total
            .checked_add(
                n.checked_mul(60)
                    .ok_or_else(|| format!("Duration is too large: {s}"))?,
            )
            .ok_or_else(|| format!("Duration is too large: {s}"))?;
    }

    if total == 0 {
        return Err("Duration cannot be zero".to_string());
    }
    Ok(total)
}

fn atomic_write_json(path: &Path, content: &str) -> std::io::Result<()> {
    let temp_path = path.with_extension("json.tmp");
    std::fs::write(&temp_path, content)?;
    std::fs::rename(temp_path, path)
}

fn parse_time(s: &str) -> Result<(u32, u32), String> {
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() != 2 {
        return Err(format!("Expected HH:MM, got: {}", s));
    }
    let h: u32 = parts[0]
        .parse()
        .map_err(|_| format!("Invalid hour: {}", parts[0]))?;
    let m: u32 = parts[1]
        .parse()
        .map_err(|_| format!("Invalid minute: {}", parts[1]))?;
    if h > 23 || m > 59 {
        return Err(format!("Time out of range: {}:{}", h, m));
    }
    Ok((h, m))
}

fn parse_day(s: &str) -> Result<u32, String> {
    match s {
        "mon" | "monday" => Ok(0),
        "tue" | "tuesday" => Ok(1),
        "wed" | "wednesday" => Ok(2),
        "thu" | "thursday" => Ok(3),
        "fri" | "friday" => Ok(4),
        "sat" | "saturday" => Ok(5),
        "sun" | "sunday" => Ok(6),
        _ => Err(format!("Unknown day: {}", s)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_interval_minutes() {
        match parse_schedule("every 30m").unwrap() {
            Schedule::Interval(s) => assert_eq!(s, 1800),
            _ => panic!("Expected Interval"),
        }
    }

    #[test]
    fn test_parse_interval_hours() {
        match parse_schedule("every 2h").unwrap() {
            Schedule::Interval(s) => assert_eq!(s, 7200),
            _ => panic!("Expected Interval"),
        }
    }

    #[test]
    fn test_parse_interval_mixed() {
        match parse_schedule("every 1h30m").unwrap() {
            Schedule::Interval(s) => assert_eq!(s, 5400),
            _ => panic!("Expected Interval"),
        }
    }

    #[test]
    fn test_parse_daily() {
        match parse_schedule("daily 09:30").unwrap() {
            Schedule::Daily { hour, minute } => {
                assert_eq!(hour, 9);
                assert_eq!(minute, 30);
            }
            _ => panic!("Expected Daily"),
        }
    }

    #[test]
    fn test_parse_weekly() {
        match parse_schedule("weekly mon 08:00").unwrap() {
            Schedule::Weekly { day, hour, minute } => {
                assert_eq!(day, 0);
                assert_eq!(hour, 8);
                assert_eq!(minute, 0);
            }
            _ => panic!("Expected Weekly"),
        }
    }

    #[test]
    fn test_reject_too_short_interval() {
        assert!(parse_schedule("every 30s").is_err());
    }

    #[test]
    fn test_reject_invalid_format() {
        assert!(parse_schedule("at midnight").is_err());
    }

    #[test]
    fn test_reject_duration_overflow() {
        assert!(parse_schedule("every 18446744073709551615d").is_err());
    }

    #[test]
    fn test_atomic_cron_write_replaces_complete_state() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cron.json");
        atomic_write_json(&path, "[1]").unwrap();
        atomic_write_json(&path, "[2]").unwrap();
        assert_eq!(std::fs::read_to_string(path).unwrap(), "[2]");
    }
}
