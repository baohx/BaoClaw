#[cfg(test)]
mod tests {
    use super::super::skills::*;
    use tempfile::tempdir;
    use tokio::fs;

    #[tokio::test]
    async fn test_discover_skills_empty() {
        let dir = tempdir().unwrap();
        let skills = discover_skills(dir.path()).await;
        let _ = skills;
    }

    #[tokio::test]
    async fn test_discover_skills_project() {
        let dir = tempdir().unwrap();
        let skills_dir = dir.path().join(".baoclaw").join("skills");
        fs::create_dir_all(&skills_dir).await.unwrap();

        fs::write(skills_dir.join("test-skill.md"), "Skill content").await.unwrap();

        let skills = discover_skills(dir.path()).await;
        let s = skills.iter().find(|s| s.name == "test-skill");
        assert!(s.is_some());
        let sk = s.unwrap();
        assert_eq!(sk.source, "project");
    }
}
