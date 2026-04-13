//! PBT: Property 6 — GrepTool result correctness
//! PBT: Property 7 — GrepTool invalid regex rejection
//!
//! **Validates: Requirements 3.1, 3.2, 3.4**
//!
//! Property 6: For any valid regex pattern and any set of text files,
//! every line returned by GrepTool shall match the regex, and every result
//! shall contain file path, line number, matched content, and context lines.
//!
//! Property 7: For any string that is not a valid regex, GrepTool shall
//! return a ToolError with a descriptive message.

use proptest::prelude::*;
use regex::Regex;

use baoclaw_core::tools::builtins::grep_tool::grep_search;

/// Strategy for generating valid, simple regex patterns that are guaranteed
/// to be compilable. We use literal strings and simple character classes.
fn valid_regex_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        // Literal word patterns
        prop::string::string_regex("[a-zA-Z]{1,10}").unwrap(),
        // Simple character class patterns
        Just("[a-z]+".to_string()),
        Just("[0-9]+".to_string()),
        Just("\\w+".to_string()),
        Just("fn \\w+".to_string()),
        Just("TODO".to_string()),
        Just("FIXME".to_string()),
    ]
}

/// Strategy for generating file content lines that may or may not match patterns.
fn file_content_strategy() -> impl Strategy<Value = String> {
    prop::collection::vec(
        prop::string::string_regex("[a-zA-Z0-9 _.,;:!?(){}\\[\\]=#/-]{0,80}")
            .unwrap(),
        1..20,
    )
    .prop_map(|lines| lines.join("\n"))
}

/// Strategy for generating invalid regex patterns.
/// All patterns here are guaranteed to be invalid — no filtering needed.
fn invalid_regex_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        // Unclosed character class
        Just("[abc".to_string()),
        Just("[".to_string()),
        Just("[z-a]".to_string()),
        // Unclosed group
        Just("(abc".to_string()),
        Just("(?P<name".to_string()),
        // Invalid repetition at start
        Just("*".to_string()),
        Just("+".to_string()),
        Just("?".to_string()),
        // Invalid escape
        Just("\\p{InvalidCategory}".to_string()),
        // Unbalanced parens
        Just("(((".to_string()),
        Just(")".to_string()),
        // Generate variations with unclosed brackets
        prop::string::string_regex("[a-z]{1,5}")
            .unwrap()
            .prop_map(|s| format!("[{}", s)),
        // Generate variations with unclosed parens
        prop::string::string_regex("[a-z]{1,5}")
            .unwrap()
            .prop_map(|s| format!("({}", s)),
    ]
}

/// Strategy for context_lines parameter
fn context_lines_strategy() -> impl Strategy<Value = usize> {
    0..5usize
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    /// Property 6: GrepTool result correctness
    ///
    /// **Validates: Requirements 3.1, 3.4**
    ///
    /// For any valid regex and file content, every returned match:
    /// - has its `content` field matching the regex
    /// - has a non-empty `file` field
    /// - has a `line_number` >= 1
    /// - has a non-empty `context` vector
    #[test]
    fn grep_results_match_regex_and_have_complete_fields(
        pattern in valid_regex_strategy(),
        content in file_content_strategy(),
        context_lines in context_lines_strategy(),
    ) {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("test_file.txt");
        std::fs::write(&file_path, &content).unwrap();

        let results = grep_search(&pattern, dir.path(), None, context_lines, 100);

        // Should not error on valid regex
        prop_assert!(results.is_ok(), "grep_search should succeed for valid regex '{}'", pattern);
        let matches = results.unwrap();

        let regex = Regex::new(&pattern).unwrap();

        for m in &matches {
            // Every returned line must match the regex
            prop_assert!(
                regex.is_match(&m.content),
                "Match content '{}' should match regex '{}'",
                m.content, pattern
            );

            // File path must be non-empty
            prop_assert!(
                !m.file.is_empty(),
                "Match file path should not be empty"
            );

            // Line number must be >= 1
            prop_assert!(
                m.line_number >= 1,
                "Line number should be >= 1, got {}",
                m.line_number
            );

            // Context must not be empty (at minimum contains the matched line itself)
            prop_assert!(
                !m.context.is_empty(),
                "Context should not be empty for match at line {}",
                m.line_number
            );
        }
    }

    /// Property 7: GrepTool invalid regex rejection
    ///
    /// **Validates: Requirement 3.2**
    ///
    /// For any invalid regex string, grep_search shall return a ToolError.
    #[test]
    fn invalid_regex_returns_tool_error(
        pattern in invalid_regex_strategy(),
    ) {
        let dir = tempfile::tempdir().unwrap();
        // Create a file so the search has something to walk
        let file_path = dir.path().join("dummy.txt");
        std::fs::write(&file_path, "some content\n").unwrap();

        let result = grep_search(&pattern, dir.path(), None, 2, 100);

        prop_assert!(
            result.is_err(),
            "grep_search should return Err for invalid regex '{}', but got Ok with {} matches",
            pattern,
            result.as_ref().map(|m| m.len()).unwrap_or(0)
        );

        let err_msg = format!("{}", result.unwrap_err());
        prop_assert!(
            err_msg.contains("Invalid regex"),
            "Error message should contain 'Invalid regex', got: '{}'",
            err_msg
        );
    }
}
