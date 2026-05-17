//! Streaming tool executor — provides real-time progress for long-running tools.

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::mpsc;

/// A chunk of streaming output from a tool execution.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StreamChunk {
    /// Tool execution ID.
    pub execution_id: String,
    /// Tool name.
    pub tool_name: String,
    /// Chunk type.
    pub chunk_type: StreamChunkType,
    /// Text content.
    pub content: String,
    /// Sequence number (0-based).
    pub seq: u32,
    /// Timestamp.
    pub timestamp: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum StreamChunkType {
    /// Tool started executing.
    Started,
    /// Partial output (intermediate progress).
    Progress,
    /// Stdout data.
    Stdout,
    /// Stderr data.
    Stderr,
    /// Tool completed successfully.
    Completed,
    /// Tool failed.
    Error,
    /// Heartbeat (keep-alive during long operations).
    Heartbeat,
}

/// Configuration for streaming execution.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StreamingConfig {
    /// Maximum chunks to buffer (backpressure).
    pub buffer_size: usize,
    /// Heartbeat interval in milliseconds (0 = disabled).
    pub heartbeat_interval_ms: u64,
    /// Maximum execution time in seconds.
    pub timeout_secs: u64,
    /// Whether to stream stdout.
    pub stream_stdout: bool,
    /// Whether to stream stderr.
    pub stream_stderr: bool,
    /// Maximum output size in bytes (truncates beyond this).
    pub max_output_bytes: usize,
}

impl Default for StreamingConfig {
    fn default() -> Self {
        Self {
            buffer_size: 256,
            heartbeat_interval_ms: 5000,
            timeout_secs: 300,
            stream_stdout: true,
            stream_stderr: true,
            max_output_bytes: 1024 * 1024, // 1MB
        }
    }
}

/// Handle for receiving streaming output from a tool execution.
pub struct StreamReader {
    receiver: mpsc::Receiver<StreamChunk>,
    execution_id: String,
}

impl StreamReader {
    /// Receive the next chunk, waiting if necessary.
    pub async fn next(&mut self) -> Option<StreamChunk> {
        self.receiver.recv().await
    }

    /// Collect all remaining chunks into a single result.
    pub async fn collect(mut self) -> StreamResult {
        let mut chunks = Vec::new();
        let mut stdout = String::new();
        let mut stderr = String::new();
        let mut success = false;
        let mut error_message = None;

        while let Some(chunk) = self.receiver.recv().await {
            match chunk.chunk_type {
                StreamChunkType::Stdout => stdout.push_str(&chunk.content),
                StreamChunkType::Stderr => stderr.push_str(&chunk.content),
                StreamChunkType::Completed => success = true,
                StreamChunkType::Error => {
                    success = false;
                    error_message = Some(chunk.content.clone());
                }
                _ => {}
            }
            chunks.push(chunk);
        }

        StreamResult {
            execution_id: self.execution_id.clone(),
            success,
            stdout,
            stderr,
            error_message,
            total_chunks: chunks.len() as u32,
        }
    }

    /// Get the execution ID.
    pub fn execution_id(&self) -> &str {
        &self.execution_id
    }
}

/// Final result of a streaming tool execution.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StreamResult {
    pub execution_id: String,
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
    pub error_message: Option<String>,
    pub total_chunks: u32,
}

/// Creates a streaming execution pair (writer, reader).
pub fn create_stream_pair(execution_id: String) -> (StreamWriter, StreamReader) {
    let (tx, rx) = mpsc::channel(256);
    let writer = StreamWriter {
        sender: tx,
        execution_id: execution_id.clone(),
        seq_counter: 0,
    };
    let reader = StreamReader {
        receiver: rx,
        execution_id,
    };
    (writer, reader)
}

/// Write end of a streaming execution — used by tool implementations.
pub struct StreamWriter {
    sender: mpsc::Sender<StreamChunk>,
    execution_id: String,
    seq_counter: u32,
}

impl StreamWriter {
    /// Get the execution ID.
    pub fn execution_id(&self) -> &str {
        &self.execution_id
    }

    /// Send a chunk of the given type.
    pub async fn send(&mut self, chunk_type: StreamChunkType, content: String) {
        let chunk = StreamChunk {
            execution_id: self.execution_id.clone(),
            tool_name: String::new(),
            chunk_type,
            content,
            seq: self.seq_counter,
            timestamp: chrono::Utc::now().to_rfc3339(),
        };
        self.seq_counter += 1;
        let _ = self.sender.send(chunk).await;
    }

    /// Send a started event.
    pub async fn started(&mut self, tool_name: &str) {
        let chunk = StreamChunk {
            execution_id: self.execution_id.clone(),
            tool_name: tool_name.to_string(),
            chunk_type: StreamChunkType::Started,
            content: String::new(),
            seq: self.seq_counter,
            timestamp: chrono::Utc::now().to_rfc3339(),
        };
        self.seq_counter += 1;
        let _ = self.sender.send(chunk).await;
    }

    /// Send a progress update.
    pub async fn progress(&mut self, message: &str) {
        self.send(StreamChunkType::Progress, message.to_string()).await;
    }

    /// Send stdout data.
    pub async fn stdout(&mut self, data: &str) {
        self.send(StreamChunkType::Stdout, data.to_string()).await;
    }

    /// Send stderr data.
    pub async fn stderr(&mut self, data: &str) {
        self.send(StreamChunkType::Stderr, data.to_string()).await;
    }

    /// Send completion event.
    pub async fn completed(&mut self) {
        self.send(StreamChunkType::Completed, String::new()).await;
    }

    /// Send error event.
    pub async fn error(&mut self, message: &str) {
        self.send(StreamChunkType::Error, message.to_string()).await;
    }

    /// Send heartbeat.
    pub async fn heartbeat(&mut self) {
        self.send(StreamChunkType::Heartbeat, String::new()).await;
    }
}

/// Streaming execution manager — orchestrates streaming tool execution.
pub struct StreamingExecutor {
    config: StreamingConfig,
}

impl StreamingExecutor {
    pub fn new(config: StreamingConfig) -> Self {
        Self { config }
    }

    /// Get the configuration.
    pub fn config(&self) -> &StreamingConfig {
        &self.config
    }

    /// Execute a command with streaming output.
    /// Returns a StreamReader for consuming chunks.
    pub fn execute_streaming(&self, tool_name: &str, command: &str) -> StreamReader {
        let execution_id = format!("{}_{}", tool_name, chrono::Utc::now().timestamp_millis());
        let (mut writer, reader) = create_stream_pair(execution_id);

        let timeout = self.config.timeout_secs;
        let max_bytes = self.config.max_output_bytes;
        let command = command.to_string();
        let tool_name = tool_name.to_string();

        tokio::spawn(async move {
            writer.started(&tool_name).await;

            let result = tokio::time::timeout(
                std::time::Duration::from_secs(timeout),
                Self::run_command(&command, &mut writer, max_bytes),
            ).await;

            match result {
                Ok(Ok(())) => writer.completed().await,
                Ok(Err(e)) => writer.error(&e).await,
                Err(_) => writer.error("Execution timed out").await,
            }
        });

        reader
    }

    async fn run_command(command: &str, writer: &mut StreamWriter, max_bytes: usize) -> Result<(), String> {
        use tokio::io::AsyncReadExt;
        use tokio::process::Command;

        let mut child = Command::new("/bin/sh")
            .arg("-c")
            .arg(command)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| format!("Failed to spawn: {}", e))?;

        let mut stdout = child.stdout.take().ok_or("No stdout")?;
        let mut stderr = child.stderr.take().ok_or("No stderr")?;

        let mut total_bytes = 0usize;
        let mut buf_out = [0u8; 4096];
        let mut buf_err = [0u8; 4096];

        // Read stdout and stderr concurrently
        loop {
            tokio::select! {
                result = stdout.read(&mut buf_out) => {
                    match result {
                        Ok(0) => break,
                        Ok(n) => {
                            total_bytes += n;
                            let text = String::from_utf8_lossy(&buf_out[..n]);
                            writer.stdout(&text).await;
                            if total_bytes >= max_bytes {
                                writer.progress(&format!("Output truncated at {} bytes", max_bytes)).await;
                                break;
                            }
                        }
                        Err(e) => {
                            writer.stderr(&format!("Read error: {}", e)).await;
                            break;
                        }
                    }
                }
                result = stderr.read(&mut buf_err) => {
                    match result {
                        Ok(0) => {},
                        Ok(n) => {
                            let text = String::from_utf8_lossy(&buf_err[..n]);
                            writer.stderr(&text).await;
                        }
                        Err(e) => {
                            writer.stderr(&format!("Stderr read error: {}", e)).await;
                        }
                    }
                }
            }
        }

        // Wait for process to finish
        let _ = child.wait().await;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_streaming_echo() {
        let executor = StreamingExecutor::new(StreamingConfig::default());
        let reader = executor.execute_streaming("test", "echo hello");
        let result = reader.collect().await;
        assert!(result.success);
        assert!(result.stdout.contains("hello"));
    }

    #[tokio::test]
    async fn test_streaming_error() {
        let executor = StreamingExecutor::new(StreamingConfig::default());
        let reader = executor.execute_streaming("test", "echo error >&2 && exit 1");
        let result = reader.collect().await;
        assert!(!result.success);
    }

    #[tokio::test]
    async fn test_stream_writer_reader() {
        let (mut writer, reader) = create_stream_pair("test_123".into());
        writer.started("test_tool").await;
        writer.progress("working...").await;
        writer.stdout("output data").await;
        writer.completed().await;

        let result = reader.collect().await;
        assert!(result.success);
        assert!(result.stdout.contains("output data"));
        assert_eq!(result.total_chunks, 4);
    }

    #[test]
    fn test_streaming_config_default() {
        let config = StreamingConfig::default();
        assert_eq!(config.buffer_size, 256);
        assert_eq!(config.timeout_secs, 300);
        assert_eq!(config.max_output_bytes, 1024 * 1024);
    }

    #[tokio::test]
    async fn test_heartbeat() {
        let (mut writer, reader) = create_stream_pair("hb_test".into());
        writer.heartbeat().await;
        writer.completed().await;
        let result = reader.collect().await;
        assert!(result.success);
    }
}
