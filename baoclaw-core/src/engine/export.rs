use chrono::Local;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A simplified transcript entry for export formatting.
/// Can be deserialized from the talkTail RPC response entries.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExportEntry {
    pub role: String,
    pub text: String,
    pub timestamp: String,
    pub turn: usize,
    #[serde(default)]
    pub tools: Option<Vec<ToolCallInfo>>,
    #[serde(default)]
    pub is_tool_result: Option<bool>,
    #[serde(default)]
    pub cost_usd: Option<f64>,
}

/// Tool call information for export.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolCallInfo {
    pub name: String,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub detail: Option<String>,
    #[serde(default)]
    pub result: Option<Value>,
}

/// Format a list of transcript entries into a Markdown document.
///
/// The output follows the design spec format:
/// - Title and session metadata
/// - Each message as a section with timestamp
/// - Tool calls as subsections under assistant messages
pub fn format_transcript_to_markdown(entries: &[ExportEntry]) -> String {
    let export_time = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

    // Count user and assistant messages (skip tool results)
    let message_count = entries
        .iter()
        .filter(|e| !e.is_tool_result.unwrap_or(false))
        .count();

    let mut md = String::new();

    // Header
    md.push_str("# BaoClaw 对话导出\n\n");
    md.push_str(&format!("**时间**: {}\n", export_time));
    md.push_str(&format!("**消息数**: {}\n", message_count));
    md.push_str("\n---\n\n");

    for entry in entries {
        // Skip tool result entries (they are shown inline with tool calls)
        if entry.is_tool_result.unwrap_or(false) {
            continue;
        }

        match entry.role.as_str() {
            "user" => {
                md.push_str(&format!("## 用户 ({})\n\n", entry.timestamp));
                md.push_str(&entry.text);
                md.push_str("\n\n");
            }
            "assistant" => {
                md.push_str(&format!("## 助手 ({})\n\n", entry.timestamp));
                if !entry.text.is_empty() {
                    md.push_str(&entry.text);
                    md.push_str("\n\n");
                }

                // Render tool calls if present
                if let Some(tools) = &entry.tools {
                    for tool in tools {
                        md.push_str(&format!("### 工具调用: {}\n\n", tool.name));
                        if let Some(detail) = &tool.detail {
                            md.push_str(&format!("{}\n\n", detail));
                        }
                        if let Some(result) = &tool.result {
                            let result_str = match result {
                                Value::String(s) => {
                                    // Truncate very long results
                                    if s.len() > 500 {
                                        format!("{}...(truncated)", &s[..500])
                                    } else {
                                        s.clone()
                                    }
                                }
                                other => {
                                    let s = serde_json::to_string_pretty(other)
                                        .unwrap_or_else(|_| other.to_string());
                                    if s.len() > 500 {
                                        format!("{}...(truncated)", &s[..500])
                                    } else {
                                        s
                                    }
                                }
                            };
                            md.push_str(&format!("**结果**: {}\n\n", result_str));
                        }
                    }
                }
            }
            _ => {
                // System messages
                md.push_str(&format!("## 系统 ({})\n\n", entry.timestamp));
                md.push_str(&entry.text);
                md.push_str("\n\n");
            }
        }

        md.push_str("---\n\n");
    }

    md
}

/// Generate a default export filename with current date.
pub fn default_export_filename() -> String {
    let date = Local::now().format("%Y%m%d-%H%M%S").to_string();
    format!("baoclaw-export-{}.md", date)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_empty_entries() {
        let entries: Vec<ExportEntry> = vec![];
        let md = format_transcript_to_markdown(&entries);
        assert!(md.contains("# BaoClaw 对话导出"));
        assert!(md.contains("**消息数**: 0"));
    }

    #[test]
    fn test_format_user_and_assistant() {
        let entries = vec![
            ExportEntry {
                role: "user".to_string(),
                text: "你好".to_string(),
                timestamp: "2024-01-15 10:30:00".to_string(),
                turn: 1,
                tools: None,
                is_tool_result: None,
                cost_usd: None,
            },
            ExportEntry {
                role: "assistant".to_string(),
                text: "你好！有什么可以帮助你的？".to_string(),
                timestamp: "2024-01-15 10:30:01".to_string(),
                turn: 2,
                tools: None,
                is_tool_result: None,
                cost_usd: Some(0.001),
            },
        ];
        let md = format_transcript_to_markdown(&entries);
        assert!(md.contains("**消息数**: 2"));
        assert!(md.contains("## 用户 (2024-01-15 10:30:00)"));
        assert!(md.contains("你好"));
        assert!(md.contains("## 助手 (2024-01-15 10:30:01)"));
        assert!(md.contains("你好！有什么可以帮助你的？"));
    }

    #[test]
    fn test_format_with_tool_calls() {
        let entries = vec![
            ExportEntry {
                role: "assistant".to_string(),
                text: "让我查看一下文件".to_string(),
                timestamp: "2024-01-15 10:30:01".to_string(),
                turn: 1,
                tools: Some(vec![ToolCallInfo {
                    name: "Bash".to_string(),
                    id: Some("tu_123".to_string()),
                    detail: Some("command: ls -la".to_string()),
                    result: Some(Value::String("file.txt\ndir/".to_string())),
                }]),
                is_tool_result: None,
                cost_usd: None,
            },
        ];
        let md = format_transcript_to_markdown(&entries);
        assert!(md.contains("### 工具调用: Bash"));
        assert!(md.contains("command: ls -la"));
        assert!(md.contains("**结果**: file.txt\ndir/"));
    }

    #[test]
    fn test_tool_results_skipped() {
        let entries = vec![
            ExportEntry {
                role: "user".to_string(),
                text: "tool result content".to_string(),
                timestamp: "2024-01-15 10:30:02".to_string(),
                turn: 2,
                tools: None,
                is_tool_result: Some(true),
                cost_usd: None,
            },
            ExportEntry {
                role: "user".to_string(),
                text: "real message".to_string(),
                timestamp: "2024-01-15 10:30:03".to_string(),
                turn: 3,
                tools: None,
                is_tool_result: None,
                cost_usd: None,
            },
        ];
        let md = format_transcript_to_markdown(&entries);
        // Tool result should be skipped, only real message counted
        assert!(md.contains("**消息数**: 1"));
        assert!(!md.contains("tool result content"));
        assert!(md.contains("real message"));
    }

    #[test]
    fn test_default_export_filename() {
        let filename = default_export_filename();
        assert!(filename.starts_with("baoclaw-export-"));
        assert!(filename.ends_with(".md"));
    }
}
