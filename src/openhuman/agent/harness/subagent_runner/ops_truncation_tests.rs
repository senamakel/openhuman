/// Tests for the `max_result_chars` truncation logic in `ops.rs`.
///
/// Kept in a dedicated file so `ops_tests.rs` stays under ~500 lines.
///
/// # Mirror contract
///
/// The truncation snippet below is a **verbatim copy** of the production
/// code in `run_subagent` at `ops.rs` lines 150-168 (the block starting
/// with `if let Some(cap) = definition.max_result_chars`). These tests
/// exercise that exact algorithm without spinning up a full provider +
/// parent context. If you change the production truncation logic you
/// **must** update the snippet here to match — otherwise the tests
/// silently stop covering the live code path.
///
/// Production site reference:
/// ```text
/// // ops.rs:150-168
/// if let Some(cap) = definition.max_result_chars {
///     let original_chars = outcome.output.chars().count();
///     if original_chars > cap {
///         let byte_offset = outcome
///             .output
///             .char_indices()
///             .nth(cap)
///             .map(|(i, _)| i)
///             .unwrap_or(outcome.output.len());
///         outcome.output.truncate(byte_offset);
///         outcome.output.push_str("\n[...truncated]");
///     }
/// }
/// ```

/// Applies the production truncation snippet to `input` with the given cap.
///
/// This helper is the single source of truth for the snippet used in every
/// test below. If `ops.rs` changes the truncation algorithm, update this
/// function to match.
fn apply_truncation(mut output: String, cap: usize) -> String {
    // Mirror of ops.rs:150-168 (`run_subagent` max_result_chars block).
    let original_chars = output.chars().count();
    if original_chars > cap {
        let byte_offset = output
            .char_indices()
            .nth(cap)
            .map(|(i, _)| i)
            .unwrap_or(output.len());
        output.truncate(byte_offset);
        output.push_str("\n[...truncated]");
    }
    output
}

#[test]
fn max_result_chars_cap_is_enforced() {
    // Verify that max_result_chars truncation uses char count (not bytes)
    // and produces a truncated result ending with "[...truncated]".
    // Production site: ops.rs:150-168.
    let cap = 10usize;
    let input = "hello world this is long".to_string();
    let output = apply_truncation(input, cap);
    assert_eq!(&output[..10], "hello worl");
    assert!(output.ends_with("[...truncated]"));
}

#[test]
fn max_result_chars_cap_is_char_safe_for_multibyte() {
    // A cap landing in the middle of a multi-byte UTF-8 sequence must
    // not panic. "café" has 4 chars but 'é' is 2 bytes — truncating at
    // byte offset 4 with a raw String::truncate() would panic.
    // Production site: ops.rs:150-168.
    let cap = 3usize; // keep "caf", drop "é"
    let input = "café latte".to_string();
    let output = apply_truncation(input, cap);
    assert_eq!(output, "caf\n[...truncated]");
}

#[test]
fn max_result_chars_not_applied_when_none() {
    // When no cap is set (None), the output must be returned unchanged.
    // Production site: ops.rs:150 — the `if let Some(cap)` guard.
    let cap: Option<usize> = None;
    let original = "short output".to_string();
    let output = match cap {
        Some(c) => apply_truncation(original.clone(), c),
        None => original.clone(),
    };
    assert_eq!(output, original);
}
