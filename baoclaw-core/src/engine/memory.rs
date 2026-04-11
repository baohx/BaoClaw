use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

const MEMORY_FILE: &str = "memory.jsonl";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum MemoryCategory {
    #[serde(rename = "fact")]
    Fact,
    #[serde(rename = "preference")]
    Preference,
    #[serde(rename = "decision")]
    Decision,
}

impl std::fmt::Display for MemoryCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Fact => write!(f, "fact"),
            Self::Preference => write!(f, "preference"),
            Self::Decision => write!(f, "decision"),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub id: String,
    pub content: String,
    pub category: MemoryCategory,
    pub created_at: String,
    pub source: String,
}

/// Persistent memory store backed by a JSONL file.
/// Supports both global (~/.baoclaw/) and project-level (<cwd>/.baoclaw/) memory.
pub struct MemoryStore {
    entries: Mutex<Vec<MemoryEntry>>,
    file_path: Mutex<PathBuf>,
}

impl MemoryStore {
    /// Load global memories from ~/.baoclaw/memory.jsonl.
    pub fn load() -> Self {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        let file_path = PathBuf::from(&home).join(".baoclaw").join(MEMORY_FILE);
        let entries = Self::read_file(&file_path);
        eprintln!("Loaded {} long-term memories from {}", entries.len(), file_path.display());
        Self {
            entries: Mutex::new(entries),
            file_path: Mutex::new(file_path),
        }
    }

    /// Load project-level memories from <cwd>/.baoclaw/memory.jsonl.
    /// Falls back to global if project dir doesn't have .baoclaw/.
    pub fn load_for_project(cwd: &std::path::Path) -> Self {
        let project_path = cwd.join(".baoclaw").join(MEMORY_FILE);
        let file_path = if cwd.join(".baoclaw").exists() {
            project_path
        } else {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
            PathBuf::from(&home).join(".baoclaw").join(MEMORY_FILE)
        };
        let entries = Self::read_file(&file_path);
        eprintln!("Loaded {} project memories from {}", entries.len(), file_path.display());
        Self {
            entries: Mutex::new(entries),
            file_path: Mutex::new(file_path),
        }
    }

    /// Switch to a different project's memory store.
    pub async fn switch_project(&self, cwd: &std::path::Path) {
        let new_path = if cwd.join(".baoclaw").exists() {
            cwd.join(".baoclaw").join(MEMORY_FILE)
        } else {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
            PathBuf::from(&home).join(".baoclaw").join(MEMORY_FILE)
        };
        let new_entries = Self::read_file(&new_path);
        eprintln!("Switched memory to {} ({} entries)", new_path.display(), new_entries.len());
        *self.entries.lock().await = new_entries;
        *self.file_path.lock().await = new_path;
    }

    fn read_file(path: &PathBuf) -> Vec<MemoryEntry> {
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => return Vec::new(),
        };
        content.lines()
            .filter(|l| !l.trim().is_empty())
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect()
    }

    fn write_all_sync(path: &PathBuf, entries: &[MemoryEntry]) {
        let lines: Vec<String> = entries.iter()
            .filter_map(|e| serde_json::to_string(e).ok())
            .collect();
        let _ = std::fs::write(path, lines.join("\n") + "\n");
    }

    /// Add a new memory entry.
    pub async fn add(&self, content: String, category: MemoryCategory, source: String) -> MemoryEntry {
        let entry = MemoryEntry {
            id: uuid::Uuid::new_v4().to_string()[..8].to_string(),
            content,
            category,
            created_at: chrono::Utc::now().to_rfc3339(),
            source,
        };
        let mut entries = self.entries.lock().await;
        entries.push(entry.clone());
        // Append to file
        if let Ok(line) = serde_json::to_string(&entry) {
            use std::io::Write;
            let fp = self.file_path.lock().await;
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&*fp) {
                let _ = writeln!(f, "{}", line);
            }
        }
        entry
    }

    /// List all memories.
    pub async fn list(&self) -> Vec<MemoryEntry> {
        self.entries.lock().await.clone()
    }

    /// Delete a memory by ID prefix.
    pub async fn delete(&self, id_prefix: &str) -> bool {
        let mut entries = self.entries.lock().await;
        let before = entries.len();
        entries.retain(|e| !e.id.starts_with(id_prefix));
        if entries.len() < before {
            let fp = self.file_path.lock().await;
            Self::write_all_sync(&fp, &entries);
            true
        } else {
            false
        }
    }

    /// Clear all memories.
    pub async fn clear(&self) -> usize {
        let mut entries = self.entries.lock().await;
        let count = entries.len();
        entries.clear();
        let fp = self.file_path.lock().await;
        let _ = std::fs::write(&*fp, "");
        count
    }

    /// Build a system prompt fragment from all memories.
    /// Returns None if no memories exist.
    pub async fn build_prompt_fragment(&self) -> Option<String> {
        let entries = self.entries.lock().await;
        if entries.is_empty() {
            return None;
        }

        let mut parts = Vec::new();
        parts.push("# Long-term Memory\n\nThe following are facts, preferences, and decisions remembered from previous conversations. Use them to provide personalized responses.\n".to_string());

        let facts: Vec<&MemoryEntry> = entries.iter().filter(|e| matches!(e.category, MemoryCategory::Fact)).collect();
        let prefs: Vec<&MemoryEntry> = entries.iter().filter(|e| matches!(e.category, MemoryCategory::Preference)).collect();
        let decisions: Vec<&MemoryEntry> = entries.iter().filter(|e| matches!(e.category, MemoryCategory::Decision)).collect();

        if !facts.is_empty() {
            parts.push("## Facts".to_string());
            for e in &facts {
                parts.push(format!("- {}", e.content));
            }
        }
        if !prefs.is_empty() {
            parts.push("\n## Preferences".to_string());
            for e in &prefs {
                parts.push(format!("- {}", e.content));
            }
        }
        if !decisions.is_empty() {
            parts.push("\n## Decisions".to_string());
            for e in &decisions {
                parts.push(format!("- {}", e.content));
            }
        }

        Some(parts.join("\n"))
    }
}

/// Parse a category string into MemoryCategory.
pub fn parse_category(s: &str) -> MemoryCategory {
    match s.to_lowercase().as_str() {
        "preference" | "pref" => MemoryCategory::Preference,
        "decision" | "dec" => MemoryCategory::Decision,
        _ => MemoryCategory::Fact,
    }
}
