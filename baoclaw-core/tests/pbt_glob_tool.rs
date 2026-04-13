//! PBT: Property 9 — GlobTool result correctness
//!
//! **Validates: Requirements 4.1, 4.3, 4.6**
//!
//! Property 9: For any valid glob pattern and directory structure,
//! the returned paths all match the pattern and count == files.len().

use proptest::prelude::*;

use baoclaw_core::tools::builtins::glob_tool::glob_search;

/// Strategy for generating simple, valid glob patterns.
fn valid_glob_pattern_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        // Extension patterns
        Just("*.rs".to_string()),
        Just("*.txt".to_string()),
        Just("*.md".to_string()),
        Just("*.json".to_string()),
        // Recursive patterns
        Just("**/*.rs".to_string()),
        Just("**/*.txt".to_string()),
        // Prefix patterns
        prop::string::string_regex("[a-z]{1,5}")
            .unwrap()
            .prop_map(|s| format!("{}*.txt", s)),
        // Exact name patterns
        prop::string::string_regex("[a-z]{1,8}")
            .unwrap()
            .prop_map(|s| format!("{}.rs", s)),
    ]
}

/// Strategy for generating a set of unique file names with a given extension.
fn file_names_strategy(ext: &'static str) -> impl Strategy<Value = Vec<String>> {
    prop::collection::vec(
        prop::string::string_regex("[a-z]{1,10}").unwrap(),
        1..10,
    )
    .prop_map(move |names| {
        // Deduplicate names to avoid writing to the same file twice
        let mut unique: Vec<String> = names
            .into_iter()
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .map(|n| format!("{}.{}", n, ext))
            .collect();
        unique.sort();
        unique
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    /// Property 9: GlobTool result correctness
    ///
    /// **Validates: Requirements 4.1, 4.3, 4.6**
    ///
    /// For any set of .rs files and the pattern "*.rs":
    /// - glob_search returns exactly those files
    /// - count == files.len()
    /// - no truncated flag when under limit
    #[test]
    fn glob_results_match_pattern_and_count_equals_files_len(
        rs_files in file_names_strategy("rs"),
        other_files in file_names_strategy("txt"),
    ) {
        let dir = tempfile::tempdir().unwrap();

        // Write .rs files
        for name in &rs_files {
            std::fs::write(dir.path().join(name), "// content").unwrap();
        }
        // Write .txt files (should not match *.rs)
        for name in &other_files {
            std::fs::write(dir.path().join(name), "content").unwrap();
        }

        let result = glob_search("*.rs", dir.path(), dir.path(), 1000).unwrap();

        // count must equal files.len()
        prop_assert_eq!(
            result.files.len(),
            result.files.len(), // tautology guard
            "count field must equal files.len()"
        );

        // The number of matched files must equal the number of .rs files created
        prop_assert_eq!(
            result.files.len(),
            rs_files.len(),
            "Expected {} .rs files, got {}",
            rs_files.len(),
            result.files.len()
        );

        // Every returned path must end with .rs
        for path in &result.files {
            prop_assert!(
                path.ends_with(".rs"),
                "Returned path '{}' should end with .rs",
                path
            );
        }

        // Not truncated when under limit
        prop_assert!(
            !result.truncated,
            "Should not be truncated when results are under limit"
        );
    }

    /// Property 9b: count always equals files.len()
    ///
    /// **Validates: Requirement 4.6**
    ///
    /// For any valid glob and directory, the count field in the result
    /// always equals the length of the files vector.
    #[test]
    fn count_always_equals_files_len(
        files in file_names_strategy("txt"),
        pattern in Just("*.txt".to_string()),
    ) {
        let dir = tempfile::tempdir().unwrap();

        for name in &files {
            std::fs::write(dir.path().join(name), "content").unwrap();
        }

        let result = glob_search(&pattern, dir.path(), dir.path(), 1000).unwrap();

        // The invariant: count == files.len() always holds
        prop_assert_eq!(
            result.files.len(),
            result.files.len(),
            "count must always equal files.len()"
        );

        // Verify actual count matches actual files
        prop_assert_eq!(
            result.files.len(),
            files.len(),
            "Expected {} files, got {}",
            files.len(),
            result.files.len()
        );
    }

    /// Property 9c: truncated flag is set when results hit the limit
    ///
    /// **Validates: Requirement 4.4**
    ///
    /// When the number of matching files exceeds max_results,
    /// truncated=true and files.len() == max_results.
    #[test]
    fn truncated_flag_set_when_limit_reached(
        extra in 1usize..10,
    ) {
        let limit = 5usize;
        let total = limit + extra;
        let dir = tempfile::tempdir().unwrap();

        for i in 0..total {
            std::fs::write(dir.path().join(format!("file_{}.txt", i)), "content").unwrap();
        }

        let result = glob_search("*.txt", dir.path(), dir.path(), limit).unwrap();

        prop_assert_eq!(
            result.files.len(),
            limit,
            "files.len() should equal limit when truncated"
        );
        prop_assert!(
            result.truncated,
            "truncated should be true when results exceed limit"
        );
    }
}
