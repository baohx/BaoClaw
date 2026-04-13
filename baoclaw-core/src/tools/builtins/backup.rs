use std::path::Path;

/// Backup a file before writing to it.
/// Creates a timestamped copy in `.baoclaw/backups/`.
/// If the file doesn't exist (new file), this is a no-op.
pub async fn backup_file_before_write(path: &Path, cwd: &Path) -> Result<(), std::io::Error> {
    if !path.exists() {
        return Ok(());
    }

    let backup_dir = cwd.join(".baoclaw").join("backups");
    tokio::fs::create_dir_all(&backup_dir).await?;

    let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");
    let filename = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy();
    let backup_path = backup_dir.join(format!("{}_{}", filename, timestamp));

    tokio::fs::copy(path, &backup_path).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_backup_existing_file() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("test.txt");
        std::fs::write(&file_path, "original content").unwrap();

        backup_file_before_write(&file_path, dir.path()).await.unwrap();

        let backup_dir = dir.path().join(".baoclaw").join("backups");
        assert!(backup_dir.exists());

        let entries: Vec<_> = std::fs::read_dir(&backup_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        assert_eq!(entries.len(), 1);

        let backup_name = entries[0].file_name().to_string_lossy().to_string();
        assert!(backup_name.starts_with("test.txt_"));

        let backup_content = std::fs::read_to_string(entries[0].path()).unwrap();
        assert_eq!(backup_content, "original content");
    }

    #[tokio::test]
    async fn test_backup_nonexistent_file_is_noop() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("nonexistent.txt");

        let result = backup_file_before_write(&file_path, dir.path()).await;
        assert!(result.is_ok());

        let backup_dir = dir.path().join(".baoclaw").join("backups");
        assert!(!backup_dir.exists());
    }
}
