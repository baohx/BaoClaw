/// ToolResultStore — persists oversized tool outputs to disk.
///
/// When a tool result exceeds a configurable threshold (default 30 KB), the
/// full content is written to a file under the session directory and only a
/// short preview is kept in the conversation context. This prevents large
/// outputs from consuming the context window.

use std::path::PathBuf;

/// Default threshold in bytes: 30 KB.
pub const DEFAULT_PERSIST_THRESHOLD: usize = 200_000;
/// Preview size in characters: 2 KB.
pub const PREVIEW_CHARS: usize = 2_000;

/// A tool result that was persisted to disk.
pub struct PersistedOutput {
    /// First `PREVIEW_CHARS` characters of the original content.
    pub preview: String,
    /// Path to the file containing the full output.
    pub file_path: PathBuf,
    /// Total characters of the original content.
    pub total_chars: usize,
}

/// Manages on-disk persistence of large tool results.
pub struct ToolResultStore {
    /// Base directory: `~/.baoclaw/sessions/{session_id}/tool-results/`
    base_dir: PathBuf,
    /// Threshold in bytes above which results are persisted.
    threshold: usize,
}

impl ToolResultStore {
    /// Create a ToolResultStore for the given session.
    ///
    /// The base directory is `~/.baoclaw/sessions/{session_id}/tool-results/`.
    /// It is created lazily on first persist.
    pub fn for_session(session_id: &str) -> Self {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        let base_dir = PathBuf::from(home)
            .join(".baoclaw")
            .join("sessions")
            .join(session_id)
            .join("tool-results");

        Self {
            base_dir,
            threshold: DEFAULT_PERSIST_THRESHOLD,
        }
    }

    /// Create with a custom base directory (useful for testing).
    pub fn with_base_dir(base_dir: PathBuf) -> Self {
        Self {
            base_dir,
            threshold: DEFAULT_PERSIST_THRESHOLD,
        }
    }

    /// Set a custom persist threshold.
    pub fn with_threshold(mut self, threshold: usize) -> Self {
        self.threshold = threshold;
        self
    }

    /// Check whether the content exceeds the persist threshold.
    pub fn should_persist(&self, content: &str) -> bool {
        content.len() > self.threshold
    }

    /// Persist `content` to disk and return metadata for the caller to build
    /// the in-context replacement text.
    ///
    /// The file is named `{tool_use_id}.txt` (sanitised) to make it easy to
    /// correlate with the tool call.
    pub fn persist(&self, content: &str, tool_use_id: &str) -> std::io::Result<PersistedOutput> {
        // Ensure base_dir exists
        std::fs::create_dir_all(&self.base_dir)?;

        // Sanitise tool_use_id for use as a filename
        let safe_id: String = tool_use_id
            .chars()
            .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
            .collect();
        let file_path = self.base_dir.join(format!("{}.txt", safe_id));

        std::fs::write(&file_path, content)?;

        let preview: String = content.chars().take(PREVIEW_CHARS).collect();
        let total_chars = content.len();

        Ok(PersistedOutput {
            preview,
            file_path,
            total_chars,
        })
    }

    /// Format a `PersistedOutput` into the replacement text that replaces the
    /// original tool output in the conversation context.
    pub fn format_persisted_output(&self, persisted: &PersistedOutput) -> String {
        let kb = persisted.total_chars / 1024;
        format!(
            "<persisted-output>\n\
             Output too large ({} KB). Full output saved to: {}\n\n\
             Preview (first 2KB):\n{}\n\
             </persisted-output>",
            kb,
            persisted.file_path.display(),
            persisted.preview,
        )
    }

    /// Convenience: persist + format in one call.
    ///
    /// Returns `None` if the content is below the threshold (no persistence needed).
    pub fn persist_and_format(&self, content: &str, tool_use_id: &str) -> Option<String> {
        if !self.should_persist(content) {
            return None;
        }
        match self.persist(content, tool_use_id) {
            Ok(persisted) => Some(self.format_persisted_output(&persisted)),
            Err(e) => {
                eprintln!("Warning: failed to persist tool result: {}", e);
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_should_persist_below_threshold() {
        let dir = TempDir::new().unwrap();
        let store = ToolResultStore::with_base_dir(dir.path().to_path_buf())
            .with_threshold(100);
        assert!(!store.should_persist("short"));
    }

    #[test]
    fn test_should_persist_above_threshold() {
        let dir = TempDir::new().unwrap();
        let store = ToolResultStore::with_base_dir(dir.path().to_path_buf())
            .with_threshold(100);
        let long = "x".repeat(200);
        assert!(store.should_persist(&long));
    }

    #[test]
    fn test_persist_creates_file() {
        let dir = TempDir::new().unwrap();
        let store = ToolResultStore::with_base_dir(dir.path().to_path_buf())
            .with_threshold(10);

        let content = "x".repeat(50);
        let result = store.persist(&content, "test-123").unwrap();

        assert!(result.file_path.exists());
        assert_eq!(std::fs::read_to_string(&result.file_path).unwrap(), content);
        assert_eq!(result.total_chars, 50);
    }

    #[test]
    fn test_persist_preview_truncation() {
        let dir = TempDir::new().unwrap();
        let store = ToolResultStore::with_base_dir(dir.path().to_path_buf())
            .with_threshold(10);

        let content = "abcdefghij".repeat(500); // 5000 chars
        let result = store.persist(&content, "test-456").unwrap();

        assert!(result.preview.len() <= PREVIEW_CHARS);
        assert!(result.preview.starts_with("abcdefghij"));
    }

    #[test]
    fn test_format_persisted_output() {
        let dir = TempDir::new().unwrap();
        let store = ToolResultStore::with_base_dir(dir.path().to_path_buf())
            .with_threshold(10);

        let content = "x".repeat(4000);
        let persisted = store.persist(&content, "tool-789").unwrap();
        let formatted = store.format_persisted_output(&persisted);

        assert!(formatted.contains("<persisted-output>"));
        assert!(formatted.contains("3 KB"));
        assert!(formatted.contains("Preview (first 2KB)"));
        assert!(formatted.contains("</persisted-output>"));
    }

    #[test]
    fn test_persist_and_format_below_threshold() {
        let dir = TempDir::new().unwrap();
        let store = ToolResultStore::with_base_dir(dir.path().to_path_buf())
            .with_threshold(1000);

        assert!(store.persist_and_format("short content", "abc").is_none());
    }

    #[test]
    fn test_persist_and_format_above_threshold() {
        let dir = TempDir::new().unwrap();
        let store = ToolResultStore::with_base_dir(dir.path().to_path_buf())
            .with_threshold(10);

        let content = "x".repeat(500);
        let result = store.persist_and_format(&content, "abc").unwrap();

        assert!(result.contains("<persisted-output>"));
    }

    #[test]
    fn test_sanitise_tool_use_id() {
        let dir = TempDir::new().unwrap();
        let store = ToolResultStore::with_base_dir(dir.path().to_path_buf())
            .with_threshold(10);

        let content = "x".repeat(50);
        let result = store.persist(&content, "tool/use:id*bad").unwrap();

        // The filename should only contain safe characters
        let filename = result.file_path.file_name().unwrap().to_string_lossy();
        assert!(!filename.contains('/'));
        assert!(!filename.contains(':'));
        assert!(!filename.contains('*'));
    }
}
