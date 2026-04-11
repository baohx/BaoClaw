use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::{BufRead, Write};
use std::path::PathBuf;

/// A single transcript record.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct TranscriptEntry {
    pub timestamp: String,
    pub entry_type: TranscriptEntryType,
    pub data: Value,
}

/// The type of a transcript entry.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum TranscriptEntryType {
    UserMessage,
    AssistantMessage,
    ToolUse,
    ToolResult,
    SystemEvent,
}

/// Session transcript writer — appends entries to a JSONL file.
pub struct TranscriptWriter {
    file: std::fs::File,
    session_id: String,
}

impl TranscriptWriter {
    /// Create or open a transcript file for the given session.
    ///
    /// The file is stored at `~/.baoclaw/sessions/{session_id}.jsonl`.
    pub fn open(session_id: &str) -> Result<Self, std::io::Error> {
        let dir = Self::sessions_dir()?;
        Self::open_in_dir(session_id, &dir)
    }

    /// Create or open a transcript file in a specific directory.
    pub fn open_in_dir(session_id: &str, dir: &PathBuf) -> Result<Self, std::io::Error> {
        std::fs::create_dir_all(dir)?;
        let path = dir.join(format!("{}.jsonl", session_id));
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        Ok(Self {
            file,
            session_id: session_id.to_string(),
        })
    }

    /// Append a single entry as a JSON line + flush.
    pub fn append(&mut self, entry: &TranscriptEntry) -> Result<(), std::io::Error> {
        let line = serde_json::to_string(entry)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        writeln!(self.file, "{}", line)?;
        self.file.flush()
    }

    /// Load all valid transcript entries for a session.
    ///
    /// Corrupted JSON lines are silently skipped.
    pub fn load(session_id: &str) -> Result<Vec<TranscriptEntry>, std::io::Error> {
        let dir = Self::sessions_dir()?;
        Self::load_from_dir(session_id, &dir)
    }

    /// Load all valid transcript entries from a specific directory.
    pub fn load_from_dir(session_id: &str, dir: &PathBuf) -> Result<Vec<TranscriptEntry>, std::io::Error> {
        let path = dir.join(format!("{}.jsonl", session_id));
        let file = std::fs::File::open(&path)?;
        let reader = std::io::BufReader::new(file);
        let entries = reader
            .lines()
            .filter_map(|line_result| {
                let line = line_result.ok()?;
                if line.trim().is_empty() {
                    return None;
                }
                serde_json::from_str::<TranscriptEntry>(&line).ok()
            })
            .collect();
        Ok(entries)
    }

    /// Get the session ID.
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Return the sessions directory path (`~/.baoclaw/sessions`).
    fn sessions_dir() -> Result<PathBuf, std::io::Error> {
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .map_err(|_| {
                std::io::Error::new(std::io::ErrorKind::NotFound, "HOME directory not found")
            })?;
        Ok(PathBuf::from(home).join(".baoclaw").join("sessions"))
    }
}

/// Rebuild a messages vector from transcript entries.
///
/// Only UserMessage and AssistantMessage entries are converted back to Messages.
/// ToolUse and ToolResult entries are skipped since they are embedded in the
/// assistant/user messages.
pub fn rebuild_messages_from_transcript(entries: &[TranscriptEntry]) -> Vec<crate::models::message::Message> {
    entries
        .iter()
        .filter(|e| matches!(e.entry_type, TranscriptEntryType::UserMessage | TranscriptEntryType::AssistantMessage))
        .filter_map(|e| serde_json::from_value::<crate::models::message::Message>(e.data.clone()).ok())
        .collect()
}


/// Find the most recent session file for a given cwd.
///
/// Sessions are stored as `{cwd_hash}-{uuid}.jsonl`.
/// This scans the sessions directory for files matching the cwd hash prefix
/// and returns the session_id of the most recently modified one.
pub fn find_latest_session_for_cwd(cwd: &str) -> Option<String> {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()?;
    let sessions_dir = PathBuf::from(home).join(".baoclaw").join("sessions");
    if !sessions_dir.is_dir() {
        return None;
    }

    // Compute cwd hash prefix (same FNV-1a algorithm as md5_simple in main.rs)
    let cwd_hash = {
        let mut h: u64 = 0xcbf29ce484222325;
        for b in cwd.bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        format!("{:x}", h)
    };
    let prefix = &cwd_hash[..8.min(cwd_hash.len())];

    let mut best: Option<(String, std::time::SystemTime)> = None;

    if let Ok(entries) = std::fs::read_dir(&sessions_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with(prefix) && name.ends_with(".jsonl") {
                let session_id = name.trim_end_matches(".jsonl").to_string();
                if let Ok(meta) = entry.metadata() {
                    if let Ok(modified) = meta.modified() {
                        if best.as_ref().map_or(true, |(_, t)| modified > *t) {
                            best = Some((session_id, modified));
                        }
                    }
                }
            }
        }
    }

    best.map(|(id, _)| id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Helper to create a test TranscriptEntry.
    fn make_entry(entry_type: TranscriptEntryType, data: Value) -> TranscriptEntry {
        TranscriptEntry {
            timestamp: chrono::Utc::now().to_rfc3339(),
            entry_type,
            data,
        }
    }

    #[test]
    fn test_transcript_entry_serialization_roundtrip() {
        let entry = make_entry(
            TranscriptEntryType::UserMessage,
            json!({"role": "user", "content": "hello"}),
        );
        let json_str = serde_json::to_string(&entry).unwrap();
        let deserialized: TranscriptEntry = serde_json::from_str(&json_str).unwrap();
        assert_eq!(entry, deserialized);
    }

    #[test]
    fn test_transcript_entry_type_variants() {
        let types = vec![
            TranscriptEntryType::UserMessage,
            TranscriptEntryType::AssistantMessage,
            TranscriptEntryType::ToolUse,
            TranscriptEntryType::ToolResult,
            TranscriptEntryType::SystemEvent,
        ];
        for t in types {
            let entry = make_entry(t.clone(), json!({}));
            let json_str = serde_json::to_string(&entry).unwrap();
            let deserialized: TranscriptEntry = serde_json::from_str(&json_str).unwrap();
            assert_eq!(entry.entry_type, deserialized.entry_type);
        }
    }

    #[test]
    fn test_write_and_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let sessions_dir = dir.path().join("sessions");

        let session_id = "test-roundtrip-session";
        let entries = vec![
            make_entry(TranscriptEntryType::UserMessage, json!({"content": "hello"})),
            make_entry(TranscriptEntryType::AssistantMessage, json!({"content": "hi there"})),
            make_entry(TranscriptEntryType::ToolUse, json!({"tool": "bash", "input": {"cmd": "ls"}})),
            make_entry(TranscriptEntryType::ToolResult, json!({"output": "file.txt"})),
        ];

        // Write entries
        {
            let mut writer = TranscriptWriter::open_in_dir(session_id, &sessions_dir).unwrap();
            assert_eq!(writer.session_id(), session_id);
            for entry in &entries {
                writer.append(entry).unwrap();
            }
        }

        // Load and verify
        let loaded = TranscriptWriter::load_from_dir(session_id, &sessions_dir).unwrap();
        assert_eq!(loaded.len(), entries.len());
        for (original, loaded_entry) in entries.iter().zip(loaded.iter()) {
            assert_eq!(original.entry_type, loaded_entry.entry_type);
            assert_eq!(original.data, loaded_entry.data);
            assert_eq!(original.timestamp, loaded_entry.timestamp);
        }
    }

    #[test]
    fn test_corrupted_lines_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let sessions_dir = dir.path().join("sessions");

        let session_id = "test-corrupted-session";

        // Write a valid entry
        {
            let mut writer = TranscriptWriter::open_in_dir(session_id, &sessions_dir).unwrap();
            let entry = make_entry(TranscriptEntryType::UserMessage, json!({"content": "valid"}));
            writer.append(&entry).unwrap();
        }

        // Manually append a corrupted line
        let path = sessions_dir.join(format!("{}.jsonl", session_id));
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
        writeln!(file, "{{this is not valid json}}").unwrap();
        writeln!(file, "").unwrap(); // empty line

        // Write another valid entry
        {
            let mut writer = TranscriptWriter::open_in_dir(session_id, &sessions_dir).unwrap();
            let entry = make_entry(TranscriptEntryType::AssistantMessage, json!({"content": "also valid"}));
            writer.append(&entry).unwrap();
        }

        // Load should skip corrupted and empty lines
        let loaded = TranscriptWriter::load_from_dir(session_id, &sessions_dir).unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].entry_type, TranscriptEntryType::UserMessage);
        assert_eq!(loaded[1].entry_type, TranscriptEntryType::AssistantMessage);
    }

    #[test]
    fn test_load_nonexistent_session_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let sessions_dir = dir.path().join("sessions");
        std::fs::create_dir_all(&sessions_dir).unwrap();

        let result = TranscriptWriter::load_from_dir("nonexistent-session-id", &sessions_dir);
        assert!(result.is_err());
    }

    #[test]
    fn test_empty_file_loads_empty_vec() {
        let dir = tempfile::tempdir().unwrap();
        let sessions_dir = dir.path().join("sessions");

        let session_id = "test-empty-session";

        // Create an empty file by opening and immediately closing
        {
            let _writer = TranscriptWriter::open_in_dir(session_id, &sessions_dir).unwrap();
        }

        let loaded = TranscriptWriter::load_from_dir(session_id, &sessions_dir).unwrap();
        assert!(loaded.is_empty());
    }

    #[test]
    fn test_rebuild_messages_from_transcript() {
        use crate::models::message::{Message, MessageContent, ApiUserMessage, ApiAssistantMessage, ContentBlock};

        let user_msg = Message {
            uuid: "550e8400-e29b-41d4-a716-446655440000".to_string(),
            timestamp: "2024-01-15T10:30:00Z".to_string(),
            content: MessageContent::User {
                message: ApiUserMessage {
                    role: "user".to_string(),
                    content: Value::String("hello".to_string()),
                },
                is_meta: false,
                tool_use_result: None,
            },
        };

        let assistant_msg = Message {
            uuid: "550e8400-e29b-41d4-a716-446655440001".to_string(),
            timestamp: "2024-01-15T10:30:01Z".to_string(),
            content: MessageContent::Assistant {
                message: ApiAssistantMessage {
                    role: "assistant".to_string(),
                    content: vec![ContentBlock::Text { text: "hi".to_string() }],
                    stop_reason: Some("end_turn".to_string()),
                    usage: None,
                },
                cost_usd: 0.001,
                duration_ms: 100,
            },
        };

        let entries = vec![
            TranscriptEntry {
                timestamp: "2024-01-15T10:30:00Z".to_string(),
                entry_type: TranscriptEntryType::UserMessage,
                data: serde_json::to_value(&user_msg).unwrap(),
            },
            TranscriptEntry {
                timestamp: "2024-01-15T10:30:01Z".to_string(),
                entry_type: TranscriptEntryType::AssistantMessage,
                data: serde_json::to_value(&assistant_msg).unwrap(),
            },
            TranscriptEntry {
                timestamp: "2024-01-15T10:30:02Z".to_string(),
                entry_type: TranscriptEntryType::ToolUse,
                data: json!({"tool_name": "bash", "input": {}}),
            },
            TranscriptEntry {
                timestamp: "2024-01-15T10:30:03Z".to_string(),
                entry_type: TranscriptEntryType::ToolResult,
                data: json!({"output": "ok"}),
            },
        ];

        let messages = rebuild_messages_from_transcript(&entries);
        // Only UserMessage and AssistantMessage entries are converted
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].uuid, user_msg.uuid);
        assert_eq!(messages[1].uuid, assistant_msg.uuid);
    }
}
