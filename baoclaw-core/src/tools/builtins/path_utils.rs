use std::path::{Path, PathBuf};

/// Resolve and validate a file path, preventing path traversal attacks.
/// Returns the normalized path.
///
/// - Both absolute and relative paths must stay within the allowed
///   working directory boundaries (cwd + additional_dirs) to prevent `..` escapes
///   and unauthorized filesystem traversal.
pub fn resolve_and_validate_path(
    path: &str,
    cwd: &Path,
    additional_dirs: &[PathBuf],
) -> Result<PathBuf, String> {
    if path.is_empty() {
        return Err("Path cannot be empty".to_string());
    }

    let raw = Path::new(path);

    let normalized = if raw.is_absolute() {
        normalize_path(raw)
    } else {
        normalize_path(&cwd.join(raw))
    };

    if !is_within_boundaries(&normalized, cwd, additional_dirs) {
        return Err(format!(
            "Path '{}' is outside the allowed working directories",
            path
        ));
    }

    if contains_symlink_escape(&normalized, cwd, additional_dirs) {
        return Err(format!(
            "Path '{}' resolves through a symlink outside the allowed working directories",
            path
        ));
    }

    Ok(normalized)
}

fn contains_symlink_escape(path: &Path, cwd: &Path, additional_dirs: &[PathBuf]) -> bool {
    let candidate = if path.exists() {
        path.to_path_buf()
    } else {
        match path.parent().and_then(|parent| parent.canonicalize().ok()) {
            Some(parent) => parent.join(path.file_name().unwrap_or_default()),
            None => return false,
        }
    };

    let resolved = match candidate.canonicalize() {
        Ok(path) => path,
        Err(_) => return false,
    };
    !is_within_canonical_boundaries(&resolved, cwd, additional_dirs)
}

fn is_within_canonical_boundaries(path: &Path, cwd: &Path, additional_dirs: &[PathBuf]) -> bool {
    let roots = std::iter::once(cwd).chain(additional_dirs.iter().map(PathBuf::as_path));
    roots
        .filter_map(|root| root.canonicalize().ok())
        .any(|root| path.starts_with(root))
}

/// Normalize a path by resolving `.` and `..` components lexically.
fn normalize_path(path: &Path) -> PathBuf {
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                // Only pop if we have a normal component to pop
                if !components.is_empty() {
                    components.pop();
                }
            }
            std::path::Component::CurDir => {
                // Skip `.`
            }
            other => {
                components.push(other);
            }
        }
    }
    components.iter().collect()
}

/// Check if a resolved path is within the allowed working directories.
fn is_within_boundaries(path: &Path, cwd: &Path, additional_dirs: &[PathBuf]) -> bool {
    let normalized_cwd = normalize_path(cwd);
    if path.starts_with(&normalized_cwd) {
        return true;
    }
    for dir in additional_dirs {
        let normalized_dir = normalize_path(dir);
        if path.starts_with(&normalized_dir) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_simple_relative_path() {
        let cwd = Path::new("/home/user/project");
        let result = resolve_and_validate_path("src/main.rs", cwd, &[]);
        assert_eq!(
            result.unwrap(),
            PathBuf::from("/home/user/project/src/main.rs")
        );
    }

    #[test]
    fn test_resolve_absolute_path_within_cwd() {
        let cwd = Path::new("/home/user/project");
        let result = resolve_and_validate_path("/home/user/project/src/main.rs", cwd, &[]);
        assert_eq!(
            result.unwrap(),
            PathBuf::from("/home/user/project/src/main.rs")
        );
    }

    #[test]
    fn test_reject_path_traversal_with_dotdot() {
        let cwd = Path::new("/home/user/project");
        let result = resolve_and_validate_path("../../etc/passwd", cwd, &[]);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("outside the allowed"));
    }

    #[test]
    fn test_reject_absolute_path_outside_cwd() {
        let cwd = Path::new("/home/user/project");
        let result = resolve_and_validate_path("/etc/passwd", cwd, &[]);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .contains("outside the allowed working directories"));
    }

    #[test]
    fn test_allow_path_in_additional_dir() {
        let cwd = Path::new("/home/user/project");
        let additional = vec![PathBuf::from("/opt/shared")];
        let result = resolve_and_validate_path("/opt/shared/data.txt", cwd, &additional);
        assert_eq!(result.unwrap(), PathBuf::from("/opt/shared/data.txt"));
    }

    #[test]
    fn test_reject_absolute_path_outside_additional_dirs() {
        let cwd = Path::new("/home/user/project");
        let additional = vec![PathBuf::from("/opt/shared")];
        let result = resolve_and_validate_path("/opt/other/data.txt", cwd, &additional);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .contains("outside the allowed working directories"));
    }

    #[test]
    fn test_reject_empty_path() {
        let cwd = Path::new("/home/user/project");
        let result = resolve_and_validate_path("", cwd, &[]);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("cannot be empty"));
    }

    #[test]
    fn test_resolve_path_with_dot_components() {
        let cwd = Path::new("/home/user/project");
        let result = resolve_and_validate_path("./src/../src/main.rs", cwd, &[]);
        assert_eq!(
            result.unwrap(),
            PathBuf::from("/home/user/project/src/main.rs")
        );
    }

    #[test]
    fn test_traversal_disguised_in_middle() {
        let cwd = Path::new("/home/user/project");
        // Goes up to /home/user then into /home/user/other — outside project
        let result = resolve_and_validate_path("../other/file.txt", cwd, &[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_dotdot_staying_within_cwd() {
        let cwd = Path::new("/home/user/project");
        // src/../lib is still within /home/user/project
        let result = resolve_and_validate_path("src/../lib/mod.rs", cwd, &[]);
        assert_eq!(
            result.unwrap(),
            PathBuf::from("/home/user/project/lib/mod.rs")
        );
    }

    #[test]
    fn test_normalize_path() {
        assert_eq!(
            normalize_path(Path::new("/a/b/../c/./d")),
            PathBuf::from("/a/c/d")
        );
        assert_eq!(normalize_path(Path::new("/a/b/c")), PathBuf::from("/a/b/c"));
    }
}
