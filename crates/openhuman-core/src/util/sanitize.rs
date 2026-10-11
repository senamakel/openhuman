//! Sanitization adapters for untrusted text bound for a model's context.
//!
//! TinyTools owns the lexical rules, shared with TinyMCP. These thin adapters
//! retain the host utility API without re-exporting an upstream implementation.

pub use tinymcp_bus::sanitize::{MAX_DESCRIPTION_BYTES, MAX_TITLE_BYTES};

/// Strip control characters through TinyTools' shared sanitization rules.
pub fn strip_control_chars(input: &str) -> String {
    tinytools_std::sanitize::strip_control_chars(input)
}

/// Remove instruction delimiters from untrusted text.
pub fn strip_instruction_fences(input: &str) -> String {
    tinytools_std::sanitize::strip_instruction_fences(input)
}

/// Apply a byte limit without splitting a UTF-8 character.
pub fn truncate_utf8_safe(input: &str, max_bytes: usize) -> String {
    tinytools_std::sanitize::truncate_utf8_safe(input, max_bytes)
}

/// Sanitize untrusted text and limit its size through the shared helper.
pub fn sanitize_for_llm(input: &str, max_bytes: usize) -> String {
    tinytools_std::sanitize::sanitize_for_llm(input, max_bytes)
}
