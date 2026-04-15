//! Project registry — manages named project sessions with descriptions.
//!
//! Stored in ~/.baoclaw/projects.json. Each project maps a cwd to a
//! session with a human-readable description.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::sync::Mutex;

const PROJECTS_FILE: &str = "projects.json";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProjectEntry {
    pub id: String,
    pub cwd: String,
    pub description: String,
    pub created_at: String,
    pub last_accessed: Option<String>,
}

pub struct ProjectRegistry {
    entries: Mutex<Vec<ProjectEntry>>,
    file_path: PathBuf,
}

impl ProjectRegistry {
    pub fn new() -> Self {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        let file_path = PathBuf::from(&home).join(".baoclaw").join(PROJECTS_FILE);
        let entries = Self::load(&file_path);
        eprintln!("Projects: loaded {} entries", entries.len());
        Self {
            entries: Mutex::new(entries),
            file_path,
        }
    }

    /// List all projects.
    pub async fn list(&self) -> Vec<ProjectEntry> {
        self.entries.lock().await.clone()
    }

    /// Find a project by cwd.
    pub async fn find_by_cwd(&self, cwd: &str) -> Option<ProjectEntry> {
        let entries = self.entries.lock().await;
        entries.iter().find(|e| e.cwd == cwd).cloned()
    }

    /// Find a project by ID prefix (must be unique match).
    pub async fn find_by_prefix(&self, prefix: &str) -> Result<ProjectEntry, String> {
        let entries = self.entries.lock().await;
        let matches: Vec<&ProjectEntry> = entries.iter()
            .filter(|e| e.id.starts_with(prefix))
            .collect();
        match matches.len() {
            0 => Err(format!("No project matching '{}'", prefix)),
            1 => Ok(matches[0].clone()),
            n => {
                let ids: Vec<&str> = matches.iter().map(|e| e.id.as_str()).collect();
                Err(format!("Ambiguous prefix '{}', matches {} projects: {}", prefix, n, ids.join(", ")))
            }
        }
    }

    /// Register a new project. Returns error if cwd already exists.
    pub async fn register(&self, cwd: String, description: String) -> Result<ProjectEntry, String> {
        let mut entries = self.entries.lock().await;

        // Check for duplicate cwd
        if let Some(existing) = entries.iter().find(|e| e.cwd == cwd) {
            return Err(format!("Project already exists: [{}] {} — use /projects {} to switch",
                existing.id, existing.description, existing.id));
        }

        let entry = ProjectEntry {
            id: uuid::Uuid::new_v4().to_string()[..8].to_string(),
            cwd,
            description,
            created_at: chrono::Utc::now().to_rfc3339(),
            last_accessed: Some(chrono::Utc::now().to_rfc3339()),
        };
        entries.push(entry.clone());
        self.save(&entries);
        Ok(entry)
    }

    /// Update last_accessed timestamp for a project.
    pub async fn touch(&self, cwd: &str) {
        let mut entries = self.entries.lock().await;
        for e in entries.iter_mut() {
            if e.cwd == cwd {
                e.last_accessed = Some(chrono::Utc::now().to_rfc3339());
                break;
            }
        }
        self.save(&entries);
    }

    /// Update description for a project.
    pub async fn update_description(&self, id_prefix: &str, description: String) -> Result<(), String> {
        let mut entries = self.entries.lock().await;
        let matches: Vec<usize> = entries.iter().enumerate()
            .filter(|(_, e)| e.id.starts_with(id_prefix))
            .map(|(i, _)| i)
            .collect();
        match matches.len() {
            0 => Err("Project not found".to_string()),
            1 => {
                entries[matches[0]].description = description;
                self.save(&entries);
                Ok(())
            }
            _ => Err("Ambiguous prefix".to_string()),
        }
    }

    /// Auto-register a project if not already registered.
    /// Returns (entry, is_new).
    pub async fn ensure_registered(&self, cwd: &str, description: Option<String>) -> (ProjectEntry, bool) {
        if let Some(entry) = self.find_by_cwd(cwd).await {
            self.touch(cwd).await;
            return (entry, false);
        }
        let desc = description.unwrap_or_else(|| {
            std::path::Path::new(cwd)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| cwd.to_string())
        });
        match self.register(cwd.to_string(), desc).await {
            Ok(entry) => (entry, true),
            Err(_) => {
                // Race condition: another thread registered it
                let entry = self.find_by_cwd(cwd).await.unwrap();
                (entry, false)
            }
        }
    }

    fn load(path: &PathBuf) -> Vec<ProjectEntry> {
        match std::fs::read_to_string(path) {
            Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
            Err(_) => Vec::new(),
        }
    }

    fn save(&self, entries: &[ProjectEntry]) {
        if let Ok(json) = serde_json::to_string_pretty(entries) {
            let _ = std::fs::write(&self.file_path, json);
        }
    }
}
