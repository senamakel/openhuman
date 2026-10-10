//! Bounded socket payloads for delegated tool inputs and outputs.

/// Upper bound on the sub-agent tool output forwarded to the drawer over
/// Socket.IO. The `SubagentToolCallCompleted` event carries the *pre-handoff*
/// tool result (the result-handoff path that stashes large toolkit payloads
/// behind a short placeholder runs later, in `SubagentToolSource`), so a raw
/// multi-MB integration result would otherwise ship in full to the socket /
/// Redux / DOM. Cap it here on a UTF-8 boundary with a truncation marker so the
/// drawer payload stays bounded while still showing what the tool returned.
pub(super) const MAX_WIRE_SUBAGENT_OUTPUT: usize = 256 * 1024;

/// Bytes reserved within the cap for the truncation marker so the *final*
/// payload (content + marker) never exceeds [`MAX_WIRE_SUBAGENT_OUTPUT`].
/// Generous upper bound for `…[truncated <N> bytes of tool output]` at any
/// plausible `N` (the "…" is 3 UTF-8 bytes).
const TRUNCATION_MARKER_BUDGET: usize = 80;

/// Truncate `output` so the returned string stays within
/// [`MAX_WIRE_SUBAGENT_OUTPUT`] bytes, slicing on a char boundary and
/// appending a marker (which is itself counted against the cap) when content
/// was dropped. Returns the input unchanged when it's already within the cap.
pub(super) fn cap_wire_output(output: String) -> String {
    if output.len() <= MAX_WIRE_SUBAGENT_OUTPUT {
        return output;
    }
    let mut end = MAX_WIRE_SUBAGENT_OUTPUT.saturating_sub(TRUNCATION_MARKER_BUDGET);
    while end > 0 && !output.is_char_boundary(end) {
        end -= 1;
    }
    let omitted = output.len() - end;
    format!(
        "{}\n…[truncated {omitted} bytes of tool output]",
        &output[..end]
    )
}

/// Cap a tool call's forwarded `args`/input payload the same way
/// [`cap_wire_output`] caps tool output: a captured argument (e.g. a large
/// inline file body) must never ship megabytes over the socket. Truncation
/// only ever produces a marker string; it never re-nests as JSON, since the
/// wire consumer only needs to know the payload was too big to show in full.
pub(super) fn cap_wire_args(args: Option<serde_json::Value>) -> Option<serde_json::Value> {
    let value = args?;
    if value.is_null() {
        return None;
    }
    let rendered = value.to_string();
    if rendered.len() <= MAX_WIRE_SUBAGENT_OUTPUT {
        return Some(value);
    }
    Some(serde_json::Value::String(cap_wire_output(rendered)))
}
