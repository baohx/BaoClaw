//! Integration test: verify TokenCounter improves accuracy over chars/4.

use baoclaw_core::engine::token_counter::TokenCounter;

#[test]
fn calibration_reduces_estimate_error() {
    // tiktoken should give us ≥ 6 tokens for this Chinese string.
    // chars/4 would give ≈ 4 (15 chars / 4 = 3.75) — clearly undercounting.
    let chinese = TokenCounter::count_text_tokens("测试中文分词的准确度和性能表现。");
    assert!(chinese >= 6, "Chinese tokens = {}, expected >= 6", chinese);

    let chars_div_4_estimate = "测试中文分词的准确度和性能表现。".chars().count() / 4;
    assert!(
        chinese as usize > chars_div_4_estimate,
        "tiktoken should over-count vs chars/4: {} vs {}",
        chinese,
        chars_div_4_estimate
    );
}

#[test]
fn english_token_count_matches_known_values() {
    // Known cl100k token counts:
    assert_eq!(TokenCounter::count_text_tokens(""), 0);
    assert_eq!(TokenCounter::count_text_tokens("Hello"), 1);
    assert_eq!(TokenCounter::count_text_tokens("Hello world"), 2);
    // "tiktoken" is 1 token, " is" is 1 token, " a" is 1 token, " tokenizer" is 1
    let n = TokenCounter::count_text_tokens("tiktoken is a tokenizer");
    assert!(n >= 4 && n <= 8, "got {}", n);
}
