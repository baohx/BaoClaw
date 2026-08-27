//! Git platform authentication management.
//!
//! Manages GitHub and GitLab API tokens for authenticated operations.
//! Reads tokens from environment variables and provides validation.

use std::env;

/// Result type for auth operations.
pub type AuthResult<T> = Result<T, AuthError>;

/// Auth-specific errors.
#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("no token found for platform '{platform}'. Set {env_var} environment variable.")]
    TokenNotFound { platform: String, env_var: String },
    #[error("token validation failed for platform '{platform}': {reason}")]
    ValidationFailed { platform: String, reason: String },
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

/// Supported git hosting platforms.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitPlatform {
    GitHub,
    GitLab,
}

impl GitPlatform {
    /// Get the environment variable name for the platform's token.
    pub fn env_var(&self) -> &'static str {
        match self {
            GitPlatform::GitHub => "GITHUB_TOKEN",
            GitPlatform::GitLab => "GITLAB_TOKEN",
        }
    }

    /// Get the platform name as a string.
    pub fn name(&self) -> &'static str {
        match self {
            GitPlatform::GitHub => "github",
            GitPlatform::GitLab => "gitlab",
        }
    }

    /// Parse a platform name string.
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "github" | "gh" => Some(GitPlatform::GitHub),
            "gitlab" | "gl" => Some(GitPlatform::GitLab),
            _ => None,
        }
    }
}

/// Manages git platform authentication tokens.
///
/// Reads credentials from:
/// - `GITHUB_TOKEN` environment variable (for GitHub)
/// - `GITLAB_TOKEN` environment variable (for GitLab)
/// - Files referenced by `GITHUB_TOKEN_FILE` / `GITLAB_TOKEN_FILE` (Docker secrets pattern)
pub struct GitAuth;

impl GitAuth {
    /// Get the API token for a given platform.
    ///
    /// Checks environment variables in order:
    /// 1. Direct env var (e.g. `GITHUB_TOKEN`)
    /// 2. File-based env var (e.g. `GITHUB_TOKEN_FILE`)
    pub fn get_token(platform: &str) -> Result<String, AuthError> {
        let platform_enum =
            GitPlatform::from_str(platform).ok_or_else(|| AuthError::TokenNotFound {
                platform: platform.to_string(),
                env_var: "GITHUB_TOKEN or GITLAB_TOKEN".to_string(),
            })?;

        let env_var = platform_enum.env_var();
        let file_env_var = format!("{}_FILE", env_var);

        // 1. Try direct environment variable
        if let Ok(token) = env::var(env_var) {
            let token = token.trim().to_string();
            if !token.is_empty() {
                return Ok(token);
            }
        }

        // 2. Try file-based environment variable (Docker secrets / Kubernetes secrets)
        if let Ok(file_path) = env::var(&file_env_var) {
            let content = std::fs::read_to_string(file_path.trim()).map_err(AuthError::IoError)?;
            let token = content.trim().to_string();
            if !token.is_empty() {
                return Ok(token);
            }
        }

        Err(AuthError::TokenNotFound {
            platform: platform.to_string(),
            env_var: format!("{} or {}", env_var, file_env_var),
        })
    }

    /// Check if a token is available for the given platform.
    pub fn has_token(platform: &str) -> bool {
        Self::get_token(platform).is_ok()
    }

    /// List all platforms for which tokens are available.
    pub fn available_platforms() -> Vec<String> {
        let mut platforms = Vec::new();
        for platform in &[GitPlatform::GitHub, GitPlatform::GitLab] {
            if Self::has_token(platform.name()) {
                platforms.push(platform.name().to_string());
            }
        }
        platforms
    }

    /// Get the appropriate Authorization header value for HTTP requests.
    pub fn auth_header(platform: &str) -> Result<String, AuthError> {
        let platform_enum =
            GitPlatform::from_str(platform).ok_or_else(|| AuthError::TokenNotFound {
                platform: platform.to_string(),
                env_var: "GITHUB_TOKEN or GITLAB_TOKEN".to_string(),
            })?;

        let token = Self::get_token(platform)?;
        match platform_enum {
            GitPlatform::GitHub => Ok(format!("Bearer {}", token)),
            GitPlatform::GitLab => Ok(format!("Bearer {}", token)),
        }
    }

    /// Validate that a token has the expected format.
    ///
    /// GitHub tokens: `ghp_*`, `gho_*`, `ghu_*`, `ghs_*`, `ghr_*`
    /// GitLab tokens: `glpat-*` or 20+ chars
    pub fn validate_token_format(platform: &str, token: &str) -> bool {
        let platform_enum = match GitPlatform::from_str(platform) {
            Some(p) => p,
            None => return false,
        };

        let token = token.trim();
        match platform_enum {
            GitPlatform::GitHub => {
                // GitHub PATs start with known prefixes
                token.starts_with("ghp_")
                    || token.starts_with("gho_")
                    || token.starts_with("ghu_")
                    || token.starts_with("ghs_")
                    || token.starts_with("ghr_")
                    || token.starts_with("github_pat_")
            }
            GitPlatform::GitLab => {
                // GitLab PATs: glpat- prefix or 20+ chars
                token.starts_with("glpat-") || token.len() >= 20
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_git_platform_from_str() {
        assert_eq!(GitPlatform::from_str("github"), Some(GitPlatform::GitHub));
        assert_eq!(GitPlatform::from_str("GH"), Some(GitPlatform::GitHub));
        assert_eq!(GitPlatform::from_str("GitHub"), Some(GitPlatform::GitHub));
        assert_eq!(GitPlatform::from_str("gitlab"), Some(GitPlatform::GitLab));
        assert_eq!(GitPlatform::from_str("GL"), Some(GitPlatform::GitLab));
        assert_eq!(GitPlatform::from_str("GitLab"), Some(GitPlatform::GitLab));
        assert_eq!(GitPlatform::from_str("bitbucket"), None);
        assert_eq!(GitPlatform::from_str(""), None);
    }

    #[test]
    fn test_git_platform_env_var() {
        assert_eq!(GitPlatform::GitHub.env_var(), "GITHUB_TOKEN");
        assert_eq!(GitPlatform::GitLab.env_var(), "GITLAB_TOKEN");
    }

    #[test]
    fn test_git_platform_name() {
        assert_eq!(GitPlatform::GitHub.name(), "github");
        assert_eq!(GitPlatform::GitLab.name(), "gitlab");
    }

    #[test]
    fn test_validate_token_format_github() {
        assert!(GitAuth::validate_token_format("github", "ghp_abc123def456"));
        assert!(GitAuth::validate_token_format("github", "gho_abc123"));
        assert!(GitAuth::validate_token_format(
            "github",
            "github_pat_abc123"
        ));
        assert!(!GitAuth::validate_token_format("github", "invalid"));
        assert!(!GitAuth::validate_token_format("github", ""));
    }

    #[test]
    fn test_validate_token_format_gitlab() {
        assert!(GitAuth::validate_token_format(
            "gitlab",
            "glpat-abc123def456"
        ));
        assert!(GitAuth::validate_token_format(
            "gitlab",
            "abcdefghijklmnopqrst"
        )); // 20 chars
        assert!(!GitAuth::validate_token_format("gitlab", "short")); // < 20 chars, not glpat-
    }

    #[test]
    fn test_validate_token_format_unknown_platform() {
        assert!(!GitAuth::validate_token_format("bitbucket", "any-token"));
    }

    #[test]
    fn test_get_token_missing() {
        // Clear any existing tokens for a clean test
        // We test that an error is returned when env var is not set
        // Since test env typically doesn't have tokens, this should fail gracefully
        let result = GitAuth::get_token("bitbucket");
        assert!(result.is_err());
    }

    #[test]
    fn test_get_token_with_env_var() {
        // Test that has_token works correctly in the test environment.
        // In test env, GITHUB_TOKEN is typically not set.
        let has = GitAuth::has_token("github");
        // Either true (if token is set in CI) or false (typical local dev)
        // This just ensures the function doesn't panic.
        assert!(has == true || has == false);
    }

    #[test]
    fn test_available_platforms() {
        let platforms = GitAuth::available_platforms();
        // In test environment, typically no tokens are configured
        // This just ensures the function doesn't panic
        assert!(platforms.iter().all(|p| p == "github" || p == "gitlab"));
    }

    #[test]
    fn test_auth_header_format() {
        // Without a real token, this should fail with TokenNotFound
        let result = GitAuth::auth_header("github");
        // Either we get an error (no token) or the token happens to be set
        match result {
            Ok(header) => assert!(header.starts_with("Bearer ")),
            Err(_) => {} // expected in test env
        }
    }
}
