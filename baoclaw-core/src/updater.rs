use serde::{Deserialize, Serialize};
use std::fmt;

/// Information about an available update.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UpdateInfo {
    pub version: String,
    pub download_url: String,
    pub release_notes: String,
}

/// Errors that can occur during the update process.
#[derive(Debug)]
pub enum UpdateError {
    NetworkError(String),
    InstallError(String),
}

impl fmt::Display for UpdateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UpdateError::NetworkError(msg) => write!(f, "network error: {}", msg),
            UpdateError::InstallError(msg) => write!(f, "install error: {}", msg),
        }
    }
}

impl std::error::Error for UpdateError {}

/// Auto-updater that checks for and applies updates.
///
/// Currently a stub — `check_update()` always returns `None` since
/// no update server is configured yet. `download_and_install()` is
/// a placeholder that returns an error.
pub struct AutoUpdater {
    pub current_version: String,
    pub update_url: String,
}

impl AutoUpdater {
    pub fn new(current_version: &str) -> Self {
        Self {
            current_version: current_version.to_string(),
            update_url: "https://updates.baoclaw.dev/api/v1/check".to_string(),
        }
    }

    /// Check if an update is available.
    /// Stub: always returns None (no update server configured yet).
    pub async fn check_update(&self) -> Option<UpdateInfo> {
        // TODO: implement actual HTTP check against self.update_url
        None
    }

    /// Download and install an update.
    /// Stub: returns an error since no update mechanism is wired yet.
    pub async fn download_and_install(&self, _info: &UpdateInfo) -> Result<(), UpdateError> {
        Err(UpdateError::InstallError(
            "Auto-update is not yet implemented. Please update manually.".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_auto_updater_new() {
        let updater = AutoUpdater::new("0.1.0");
        assert_eq!(updater.current_version, "0.1.0");
        assert!(!updater.update_url.is_empty());
    }

    #[tokio::test]
    async fn test_check_update_returns_none() {
        let updater = AutoUpdater::new("0.1.0");
        let result = updater.check_update().await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_download_and_install_returns_error() {
        let updater = AutoUpdater::new("0.1.0");
        let info = UpdateInfo {
            version: "0.2.0".to_string(),
            download_url: "https://example.com/update".to_string(),
            release_notes: "Bug fixes".to_string(),
        };
        let result = updater.download_and_install(&info).await;
        assert!(result.is_err());
    }

    #[test]
    fn test_update_info_serialization() {
        let info = UpdateInfo {
            version: "1.0.0".to_string(),
            download_url: "https://example.com/v1".to_string(),
            release_notes: "Initial release".to_string(),
        };
        let json = serde_json::to_string(&info).unwrap();
        let rt: UpdateInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(rt.version, "1.0.0");
        assert_eq!(rt.download_url, "https://example.com/v1");
        assert_eq!(rt.release_notes, "Initial release");
    }

    #[test]
    fn test_update_error_display() {
        let e = UpdateError::NetworkError("timeout".to_string());
        assert_eq!(format!("{}", e), "network error: timeout");

        let e = UpdateError::InstallError("permission denied".to_string());
        assert_eq!(format!("{}", e), "install error: permission denied");
    }
}
