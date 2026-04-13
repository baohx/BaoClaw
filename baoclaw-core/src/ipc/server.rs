use std::path::{Path, PathBuf};

use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::net::UnixListener;

use super::protocol::*;

/// IPC error types
#[derive(Debug, thiserror::Error)]
pub enum IpcError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON parse error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Connection closed")]
    ConnectionClosed,
}

/// IPC server listening on a Unix Domain Socket
pub struct IpcServer {
    listener: UnixListener,
    socket_path: PathBuf,
}

/// A single IPC connection with buffered read/write
pub struct IpcConnection {
    reader: BufReader<tokio::net::unix::OwnedReadHalf>,
    writer: BufWriter<tokio::net::unix::OwnedWriteHalf>,
}

impl IpcServer {
    /// Bind to a Unix Domain Socket at the given path.
    /// Sets file permissions to 0600 (owner-only access).
    pub async fn bind(socket_path: &Path) -> std::io::Result<Self> {
        // Remove existing socket file if present
        let _ = std::fs::remove_file(socket_path);

        let listener = UnixListener::bind(socket_path)?;

        // Set socket file permissions to 0600
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o600);
            std::fs::set_permissions(socket_path, perms)?;
        }

        Ok(Self {
            listener,
            socket_path: socket_path.to_path_buf(),
        })
    }

    /// Accept a new connection, wrapping it in BufReader/BufWriter.
    pub async fn accept(&self) -> std::io::Result<IpcConnection> {
        let (stream, _addr) = self.listener.accept().await?;
        let (read_half, write_half) = stream.into_split();
        Ok(IpcConnection {
            reader: BufReader::new(read_half),
            writer: BufWriter::new(write_half),
        })
    }

    /// Returns the socket path.
    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }
}

impl Drop for IpcServer {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.socket_path);
    }
}

impl IpcConnection {
    /// Send a JSON-RPC success response as NDJSON.
    pub async fn send_response(&mut self, id: RequestId, result: Value) -> std::io::Result<()> {
        let response = JsonRpcResponse::success(id, result);
        let bytes = encode_ndjson(&response)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        self.writer.write_all(&bytes).await?;
        self.writer.flush().await
    }

    /// Send a JSON-RPC error response as NDJSON.
    pub async fn send_error(
        &mut self,
        id: Option<RequestId>,
        code: i32,
        message: String,
    ) -> std::io::Result<()> {
        let error_response = JsonRpcErrorResponse::new(id, code, message);
        let bytes = encode_ndjson(&error_response)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        self.writer.write_all(&bytes).await?;
        self.writer.flush().await
    }

    /// Send a JSON-RPC notification as NDJSON.
    pub async fn send_notification(
        &mut self,
        method: &str,
        params: Value,
    ) -> std::io::Result<()> {
        let notification = JsonRpcNotification::new(method, params);
        let bytes = encode_ndjson(&notification)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        self.writer.write_all(&bytes).await?;
        self.writer.flush().await
    }

    /// Read one NDJSON line and parse it as a JsonRpcMessage.
    pub async fn recv_message(&mut self) -> Result<JsonRpcMessage, IpcError> {
        let mut line = String::new();
        let bytes_read = self.reader.read_line(&mut line).await?;
        if bytes_read == 0 {
            return Err(IpcError::ConnectionClosed);
        }
        let message = decode_ndjson_line(&line)?;
        Ok(message)
    }

    /// Flush the write buffer.
    pub async fn flush(&mut self) -> std::io::Result<()> {
        self.writer.flush().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::UnixStream;

    #[tokio::test]
    async fn test_ipc_server_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let socket_path = dir.path().join("test.sock");

        // 1. Create IpcServer
        let server = IpcServer::bind(&socket_path).await.unwrap();
        assert!(socket_path.exists());

        // Verify socket permissions are 0600
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let meta = std::fs::metadata(&socket_path).unwrap();
            let mode = meta.permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "Socket permissions should be 0600");
        }

        // 2. Connect a client
        let client_stream = UnixStream::connect(&socket_path).await.unwrap();
        let mut conn = server.accept().await.unwrap();

        let (client_read, mut client_write) = client_stream.into_split();
        let mut client_reader = BufReader::new(client_read);

        // 3. Client sends a request, server receives it
        let request = json!({
            "jsonrpc": "2.0",
            "method": "initialize",
            "params": {"cwd": "/tmp"},
            "id": 1
        });
        let mut request_bytes = serde_json::to_vec(&request).unwrap();
        request_bytes.push(b'\n');
        client_write.write_all(&request_bytes).await.unwrap();
        client_write.flush().await.unwrap();

        let msg = conn.recv_message().await.unwrap();
        match &msg {
            JsonRpcMessage::Request(req) => {
                assert_eq!(req.method, "initialize");
                assert_eq!(req.id, RequestId::Number(1));
                assert_eq!(req.params["cwd"], "/tmp");
            }
            _ => panic!("Expected Request, got {:?}", msg),
        }

        // 4. Server sends a response, client receives it
        conn.send_response(RequestId::Number(1), json!({"capabilities": {}}))
            .await
            .unwrap();

        let mut response_line = String::new();
        client_reader.read_line(&mut response_line).await.unwrap();
        let response_msg: JsonRpcMessage = serde_json::from_str(response_line.trim()).unwrap();
        match &response_msg {
            JsonRpcMessage::Response(resp) => {
                assert_eq!(resp.id, RequestId::Number(1));
                assert_eq!(resp.result, json!({"capabilities": {}}));
            }
            _ => panic!("Expected Response, got {:?}", response_msg),
        }

        // 5. Server sends a notification, client receives it
        conn.send_notification("stream/event", json!({"type": "assistant_chunk", "content": "Hello"}))
            .await
            .unwrap();

        let mut notif_line = String::new();
        client_reader.read_line(&mut notif_line).await.unwrap();
        let notif_msg: JsonRpcMessage = serde_json::from_str(notif_line.trim()).unwrap();
        match &notif_msg {
            JsonRpcMessage::Notification(notif) => {
                assert_eq!(notif.method, "stream/event");
                assert_eq!(notif.params["type"], "assistant_chunk");
                assert_eq!(notif.params["content"], "Hello");
            }
            _ => panic!("Expected Notification, got {:?}", notif_msg),
        }
    }

    #[tokio::test]
    async fn test_ipc_connection_closed() {
        let dir = tempfile::tempdir().unwrap();
        let socket_path = dir.path().join("closed.sock");

        let server = IpcServer::bind(&socket_path).await.unwrap();
        let client_stream = UnixStream::connect(&socket_path).await.unwrap();
        let mut conn = server.accept().await.unwrap();

        // Drop the client to close the connection
        drop(client_stream);

        let result = conn.recv_message().await;
        assert!(matches!(result, Err(IpcError::ConnectionClosed)));
    }

    #[tokio::test]
    async fn test_ipc_send_error_response() {
        let dir = tempfile::tempdir().unwrap();
        let socket_path = dir.path().join("error.sock");

        let server = IpcServer::bind(&socket_path).await.unwrap();
        let client_stream = UnixStream::connect(&socket_path).await.unwrap();
        let mut conn = server.accept().await.unwrap();

        let (client_read, _client_write) = client_stream.into_split();
        let mut client_reader = BufReader::new(client_read);

        conn.send_error(Some(RequestId::Number(5)), -32601, "Method not found".into())
            .await
            .unwrap();

        let mut line = String::new();
        client_reader.read_line(&mut line).await.unwrap();
        let msg: JsonRpcMessage = serde_json::from_str(line.trim()).unwrap();
        match &msg {
            JsonRpcMessage::ErrorResponse(err) => {
                assert_eq!(err.error.code, -32601);
                assert_eq!(err.error.message, "Method not found");
                assert_eq!(err.id, Some(RequestId::Number(5)));
            }
            _ => panic!("Expected ErrorResponse, got {:?}", msg),
        }
    }

    #[tokio::test]
    async fn test_ipc_server_socket_path() {
        let dir = tempfile::tempdir().unwrap();
        let socket_path = dir.path().join("path.sock");

        let server = IpcServer::bind(&socket_path).await.unwrap();
        assert_eq!(server.socket_path(), socket_path);
    }

    #[tokio::test]
    async fn test_ipc_server_drop_cleans_up_socket() {
        let dir = tempfile::tempdir().unwrap();
        let socket_path = dir.path().join("drop.sock");

        {
            let _server = IpcServer::bind(&socket_path).await.unwrap();
            assert!(socket_path.exists());
        }
        // After drop, socket file should be removed
        assert!(!socket_path.exists());
    }
}
