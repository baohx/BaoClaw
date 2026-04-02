//! PBT: Property 13 — Session transcript round trip
//!
//! **Validates: Requirements 6.1, 6.3**
//!
//! For any sequence of TranscriptEntry values, writing them via
//! TranscriptWriter::append then loading via TranscriptWriter::load
//! shall produce an equivalent sequence of entries.

use proptest::prelude::*;
use serde_json::{json, Value};

use claude_core::engine::transcript::{
    TranscriptEntry, TranscriptEntryType, TranscriptWriter,
};

/// Strategy for generating a TranscriptEntryType.
fn entry_type_strategy() -> impl Strategy<Value = TranscriptEntryType> {
    prop_oneof![
        Just(TranscriptEntryType::UserMessage),
        Just(TranscriptEntryType::AssistantMessage),
        Just(TranscriptEntryType::ToolUse),
        Just(TranscriptEntryType::ToolResult),
        Just(TranscriptEntryType::SystemEvent),
    ]
}

/// Strategy for generating JSON data values (simple but representative).
fn data_strategy() -> impl Strategy<Value = Value> {
    prop_oneof![
        Just(json!({})),
        Just(json!({"content": "hello"})),
        Just(json!({"role": "user", "text": "test message"})),
        Just(json!({"tool": "bash", "input": {"command": "ls"}})),
        Just(json!({"output": "result", "is_error": false})),
        "[a-zA-Z0-9 ]{0,50}".prop_map(|s| json!({"text": s})),
        (0u64..1000).prop_map(|n| json!({"tokens": n})),
    ]
}

/// Strategy for generating a valid ISO 8601 timestamp.
fn timestamp_strategy() -> impl Strategy<Value = String> {
    (2020u32..2030, 1u32..13, 1u32..29, 0u32..24, 0u32..60, 0u32..60)
        .prop_map(|(y, m, d, h, min, s)| {
            format!(
                "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
                y, m, d, h, min, s
            )
        })
}

/// Strategy for generating a single TranscriptEntry.
fn entry_strategy() -> impl Strategy<Value = TranscriptEntry> {
    (timestamp_strategy(), entry_type_strategy(), data_strategy()).prop_map(
        |(timestamp, entry_type, data)| TranscriptEntry {
            timestamp,
            entry_type,
            data,
        },
    )
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// Property 13: Session transcript round trip
    ///
    /// **Validates: Requirements 6.1, 6.3**
    ///
    /// For any sequence of TranscriptEntry values, append then load
    /// produces an equivalent sequence.
    #[test]
    fn transcript_round_trip(
        entries in prop::collection::vec(entry_strategy(), 0..30),
    ) {
        let dir = tempfile::tempdir().unwrap();
        let sessions_dir = dir.path().join("sessions");
        let session_id = "pbt-roundtrip";

        // Write all entries
        {
            let mut writer = TranscriptWriter::open_in_dir(session_id, &sessions_dir).unwrap();
            for entry in &entries {
                writer.append(entry).unwrap();
            }
        }

        // Load entries back
        let loaded = TranscriptWriter::load_from_dir(session_id, &sessions_dir).unwrap();

        // Verify count matches
        prop_assert_eq!(
            loaded.len(),
            entries.len(),
            "Loaded entry count should match written count"
        );

        // Verify each entry matches
        for (i, (original, loaded_entry)) in entries.iter().zip(loaded.iter()).enumerate() {
            prop_assert_eq!(
                &original.timestamp,
                &loaded_entry.timestamp,
                "Entry {} timestamp mismatch",
                i
            );
            prop_assert_eq!(
                &original.entry_type,
                &loaded_entry.entry_type,
                "Entry {} entry_type mismatch",
                i
            );
            prop_assert_eq!(
                &original.data,
                &loaded_entry.data,
                "Entry {} data mismatch",
                i
            );
        }
    }
}
