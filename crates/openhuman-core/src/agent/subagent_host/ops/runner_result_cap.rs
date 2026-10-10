//! Character-safe result caps for delegated agent output.

/// Truncate `output` in place to the definition's `max_result_chars` cap (when
/// set), appending a `[...truncated]` marker. Char-count based (not byte-length)
/// to avoid panicking on a multi-byte UTF-8 sequence at the boundary.
pub(super) fn apply_max_result_chars(output: &mut String, cap: Option<usize>, agent_id: &str) {
    let Some(cap) = cap else { return };
    let original_chars = output.chars().count();
    if original_chars <= cap {
        return;
    }
    tracing::debug!(
        agent_id = %agent_id,
        original_chars,
        cap,
        "[subagent_host] truncating oversized result to max_result_chars cap"
    );
    let byte_offset = output
        .char_indices()
        .nth(cap)
        .map(|(i, _)| i)
        .unwrap_or(output.len());
    output.truncate(byte_offset);
    output.push_str("\n[...truncated]");
}
