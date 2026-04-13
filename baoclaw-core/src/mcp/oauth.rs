use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use super::client::McpError;

/// OAuth 2.0 authentication manager for MCP servers.
pub struct McpOAuthManager {
    token_store_dir: PathBuf,
}

/// Stored OAuth token for an MCP server.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OAuthToken {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<i64>,
    pub server_name: String,
}

impl McpOAuthManager {
    /// Create a new manager with default token store at ~/.baoclaw/mcp-auth/
    pub fn new() -> Self {
        let dir = std::env::var("HOME")
            .ok()
            .map(PathBuf::from)
            .unwrap_or_default()
            .join(".baoclaw")
            .join("mcp-auth");
        Self { token_store_dir: dir }
    }

    /// Create with a custom token store directory (for testing).
    pub fn with_dir(dir: PathBuf) -> Self {
        Self { token_store_dir: dir }
    }

    /// Get a stored token for the given server.
    pub fn get_token(&self, server_name: &str) -> Option<OAuthToken> {
        let path = self.token_store_dir.join(format!("{}.json", server_name));
        std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
    }

    /// Store a token to disk.
    pub fn store_token(&self, token: &OAuthToken) -> Result<(), std::io::Error> {
        std::fs::create_dir_all(&self.token_store_dir)?;
        let path = self
            .token_store_dir
            .join(format!("{}.json", token.server_name));
        let json = serde_json::to_string_pretty(token)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

        // Write file with restricted permissions on Unix
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            let mut opts = std::fs::OpenOptions::new();
            opts.write(true).create(true).truncate(true).mode(0o600);
            use std::io::Write;
            let mut file = opts.open(&path)?;
            file.write_all(json.as_bytes())?;
            return Ok(());
        }

        #[cfg(not(unix))]
        {
            std::fs::write(&path, json)?;
            Ok(())
        }
    }

    /// Check if a token is expired.
    pub fn is_expired(&self, token: &OAuthToken) -> bool {
        token
            .expires_at
            .map(|exp| chrono::Utc::now().timestamp() >= exp)
            .unwrap_or(false)
    }

    /// Execute OAuth 2.0 authorization code flow (stub).
    pub async fn authorize(
        &self,
        _auth_url: &str,
        _token_url: &str,
        _client_id: &str,
        _server_name: &str,
    ) -> Result<OAuthToken, McpError> {
        // 1. Start local HTTP server to listen for callback
        // 2. Open browser to auth_url
        // 3. Wait for callback with authorization_code
        // 4. Exchange code for access_token
        // 5. Store token
        todo!("OAuth flow implementation")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_manager() -> (McpOAuthManager, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let manager = McpOAuthManager::with_dir(dir.path().to_path_buf());
        (manager, dir)
    }

    fn sample_token(server_name: &str) -> OAuthToken {
        OAuthToken {
            access_token: "test-access-token-123".to_string(),
            refresh_token: Some("test-refresh-token-456".to_string()),
            expires_at: Some(chrono::Utc::now().timestamp() + 3600),
            server_name: server_name.to_string(),
        }
    }

    #[test]
    fn test_store_and_get_token() {
        let (manager, _dir) = temp_manager();
        let token = sample_token("my-server");
        manager.store_token(&token).unwrap();

        let loaded = manager.get_token("my-server").unwrap();
        assert_eq!(loaded.access_token, "test-access-token-123");
        assert_eq!(loaded.refresh_token, Some("test-refresh-token-456".to_string()));
        assert_eq!(loaded.server_name, "my-server");
    }

    #[test]
    fn test_get_token_not_found() {
        let (manager, _dir) = temp_manager();
        assert!(manager.get_token("nonexistent").is_none());
    }

    #[test]
    fn test_is_expired_future_token() {
        let (manager, _dir) = temp_manager();
        let token = OAuthToken {
            access_token: "tok".to_string(),
            refresh_token: None,
            expires_at: Some(chrono::Utc::now().timestamp() + 3600),
            server_name: "srv".to_string(),
        };
        assert!(!manager.is_expired(&token));
    }

    #[test]
    fn test_is_expired_past_token() {
        let (manager, _dir) = temp_manager();
        let token = OAuthToken {
            access_token: "tok".to_string(),
            refresh_token: None,
            expires_at: Some(chrono::Utc::now().timestamp() - 100),
            server_name: "srv".to_string(),
        };
        assert!(manager.is_expired(&token));
    }

    #[test]
    fn test_is_expired_no_expiry() {
        let (manager, _dir) = temp_manager();
        let token = OAuthToken {
            access_token: "tok".to_string(),
            refresh_token: None,
            expires_at: None,
            server_name: "srv".to_string(),
        };
        assert!(!manager.is_expired(&token));
    }

    #[test]
    fn test_store_overwrites_existing() {
        let (manager, _dir) = temp_manager();
        let token1 = OAuthToken {
            access_token: "old-token".to_string(),
            refresh_token: None,
            expires_at: None,
            server_name: "srv".to_string(),
        };
        manager.store_token(&token1).unwrap();

        let token2 = OAuthToken {
            access_token: "new-token".to_string(),
            refresh_token: Some("refresh".to_string()),
            expires_at: Some(9999999999),
            server_name: "srv".to_string(),
        };
        manager.store_token(&token2).unwrap();

        let loaded = manager.get_token("srv").unwrap();
        assert_eq!(loaded.access_token, "new-token");
    }

    #[test]
    fn test_multiple_servers() {
        let (manager, _dir) = temp_manager();
        manager.store_token(&sample_token("server-a")).unwrap();
        manager.store_token(&sample_token("server-b")).unwrap();

        assert!(manager.get_token("server-a").is_some());
        assert!(manager.get_token("server-b").is_some());
        assert!(manager.get_token("server-c").is_none());
    }

    #[cfg(unix)]
    #[test]
    fn test_token_file_permissions() {
        use std::os::unix::fs::MetadataExt;
        let (manager, _dir) = temp_manager();
        manager.store_token(&sample_token("perm-test")).unwrap();

        let path = manager.token_store_dir.join("perm-test.json");
        let metadata = std::fs::metadata(&path).unwrap();
        let mode = metadata.mode() & 0o777;
        assert_eq!(mode, 0o600, "Token file should have 0600 permissions");
    }
}
