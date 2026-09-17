use super::*;

/// Heuristic: does `text` already contain a clear, answerable question in its
/// final paragraph? Conservative by design (issue: builder convergence) — a
/// false negative (an actual question this misses) no longer discards the
/// model's text (see `combine_trail_off_fallback`), so the safe failure mode
/// stays "add a guaranteed question on top", never "under-detect and stay
/// silent".
///
/// Regression (#4887 follow-up): the original version only checked for a `?`
/// at the very end of the text / last line, which false-negatived on the
/// extremely common LLM pattern "What's X? You can find it at Y." — a real
/// question immediately followed by a trailing instructional sentence. The
/// backstop then clobbered a specific, answerable question with a generic
/// fallback. To catch that shape, this now also scans the LAST non-empty
/// paragraph for a `?` that isn't inside inline code or a fenced code block
/// (so a literal `?` in a code sample, e.g. `WHERE id = ?`, doesn't count).
///
/// Note: the trailing-noise strip below deliberately does NOT include the
/// backtick. Stripping a trailing backtick would peel off the CLOSING
/// delimiter of a code span whose last character is `?` (e.g. `` `id = ?` ``
/// at the very end of the text), exposing that `?` as if it were a bare
/// trailing question mark and defeating the code guard entirely.
pub(super) fn text_looks_like_question(text: &str) -> bool {
    let trimmed = text
        .trim()
        .trim_end_matches(['"', '\'', ')', ']', '*', '_', '.'])
        .trim_end();
    if trimmed.is_empty() {
        return false;
    }
    if trimmed.ends_with('?') {
        return true;
    }
    // The question may not be the literal last character (trailing markdown
    // like a closing code fence or list marker on its own line) — fall back
    // to the last non-blank line.
    if trimmed
        .lines()
        .rfind(|line| !line.trim().is_empty())
        .is_some_and(|last_line| last_line.trim_end().ends_with('?'))
    {
        return true;
    }
    // Final-paragraph scan: a question can sit mid-paragraph, followed by a
    // further trailing sentence on the SAME line/paragraph ("...ID? You can
    // find it under Profile > Copy member ID."). Take the last non-blank
    // paragraph and accept it if it contains a `?` that isn't inside inline
    // code / a code fence.
    last_paragraph(trimmed)
        .as_deref()
        .is_some_and(question_mark_outside_code)
}

/// Returns the last non-blank paragraph of `text` — a maximal run of
/// consecutive non-blank lines, working backward from the end and skipping
/// any trailing blank lines first. `None` if `text` has no non-blank lines.
///
/// CodeRabbit review follow-up: this used to split on the literal `"\n\n"`
/// byte sequence, which mishandles two real shapes:
/// - **CRLF input** (`"question?\r\n\r\nstatus"`): the separator is
///   `"\r\n\r\n"`, not `"\n\n"`, so the whole text was treated as ONE
///   paragraph — an earlier question could then suppress the fallback for a
///   trailing non-question status paragraph.
/// - **Whitespace-only separator lines** (`"question?\n \nstatus"` — a blank
///   line that isn't perfectly empty): same failure, same reason.
///
/// Working line-by-line via [`str::lines`] (which normalizes CRLF) and
/// treating any all-whitespace line as blank fixes both.
fn last_paragraph(text: &str) -> Option<String> {
    let mut collected: Vec<&str> = Vec::new();
    for line in text.lines().rev() {
        if line.trim().is_empty() {
            if collected.is_empty() {
                continue; // still skipping trailing blank lines
            }
            break; // blank line marks the start of the paragraph above
        }
        collected.push(line);
    }
    if collected.is_empty() {
        return None;
    }
    collected.reverse();
    Some(collected.join("\n"))
}

/// Does `text` contain at least one *sentence-terminal* `?` that isn't
/// inside a backtick-delimited code span (inline code like `` `U...` `` or a
/// fenced block like `` ``` ``)? Follows the CommonMark code-span rule: a
/// *run* of one or more consecutive backticks opens a span, and that span is
/// closed only by the next run of the SAME length — a shorter or longer run
/// of backticks encountered while inside a span is just literal backtick
/// characters, not a delimiter.
///
/// CodeRabbit review follow-up: an earlier version tracked a running
/// per-character backtick COUNT and used its parity (even = outside code).
/// That misclassifies any multi-backtick span whose delimiter is more than
/// one backtick — e.g. ``` ``SELECT ? FROM t`` ``` opens with a 2-backtick
/// run (count 0→2, even → looks "outside" again immediately), so the `?`
/// inside a valid double-backtick span was wrongly treated as outside code.
/// Tracking delimiter run LENGTH (not raw backtick count) fixes this while
/// still handling the common single-backtick and triple-backtick-fence
/// cases, since those are just the run-length-1 and run-length-3 instances
/// of the same rule.
///
/// Codex review follow-up: a bare `?` outside code isn't necessarily a real
/// question — a status line like "Checked https://api.example/search?q=foo
/// and got 403." has one mid-token, in a URL query string. Counting that
/// would flip `text_looks_like_question` to `true` and skip
/// `combine_trail_off_fallback` entirely, leaving the user with an
/// unanswerable status note — exactly the failure mode this backstop exists
/// to prevent. So each candidate `?` is additionally required to be
/// sentence-terminal via [`is_sentence_terminal_question_mark`].
pub(super) fn question_mark_outside_code(text: &str) -> bool {
    let chars: Vec<char> = text.chars().collect();
    // `Some(n)` while scanning is inside a code span opened by a run of `n`
    // backticks; that span closes only on the next run of exactly `n`.
    let mut open_run_len: Option<usize> = None;
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '`' {
            let start = i;
            while i < chars.len() && chars[i] == '`' {
                i += 1;
            }
            let run_len = i - start;
            open_run_len = match open_run_len {
                None => Some(run_len),
                Some(n) if n == run_len => None,
                Some(n) => Some(n), // mismatched run length: still inside the span
            };
            continue;
        }
        if chars[i] == '?'
            && open_run_len.is_none()
            && is_sentence_terminal_question_mark(&chars, i)
        {
            return true;
        }
        i += 1;
    }
    false
}

/// Is the `?` at `chars[index]` sentence-terminal — i.e. does it read as an
/// actual question mark rather than a character that merely happens to be a
/// `?` mid-token (a URL query string like `search?q=foo`, a shell glob,
/// etc.)? Skips over any immediately-following closing quote/bracket
/// punctuation (`"`, `'`, right single/double quotes, `)`, `]`) and requires
/// what remains to be whitespace or the end of the text — the shape a `?`
/// takes at the end of a real sentence or clause.
fn is_sentence_terminal_question_mark(chars: &[char], index: usize) -> bool {
    let mut i = index + 1;
    while let Some(&c) = chars.get(i) {
        if matches!(c, '"' | '\'' | '\u{2019}' | '\u{201D}' | ')' | ']') {
            i += 1;
            continue;
        }
        return c.is_whitespace();
    }
    true // '?' was the last character in the paragraph.
}

/// Builder-authoring tools whose result body can explain a trail-off — the
/// authoring belt `dry_run_workflow`/`validate_workflow`/`propose_workflow`/
/// `revise_workflow`/`edit_workflow`/`save_workflow` all report either a hard
/// gate rejection (`ToolResult::error`) or a self-reported broken-graph
/// result (`"ok": false` in a successful body), so a plain-text read-only
/// tool's output is never misattributed as the blocker.
const TRAIL_OFF_BLOCKER_TOOLS: &[&str] = &[
    "dry_run_workflow",
    "validate_workflow",
    "propose_workflow",
    "revise_workflow",
    "edit_workflow",
    "save_workflow",
];

/// Synthesizes a guaranteed, user-facing fallback for a trail-off turn (no
/// proposal, not capped, no run error, and the model's own text isn't a
/// question). Scans the run's tool history for the last builder-tool result
/// that looks like a blocker (a hard-gate rejection, or a `dry_run_workflow`/
/// `validate_workflow` report with `"ok": false`) and asks the user about it;
/// falls back to a generic "what should I focus on" question when no such
/// blocker is found (the model may have simply stopped with nothing to point
/// to).
pub(super) fn build_trail_off_fallback(
    history: &[crate::agent::messages::ConversationMessage],
) -> String {
    match last_builder_tool_blocker(history) {
        Some(blocker) => format!(
            "I wasn't able to finish building this workflow. Here's where I got stuck:\n\n{blocker}\n\n\
             Could you tell me how you'd like me to resolve that, or share more detail about what's needed here?"
        ),
        None => "I wasn't able to finish building this workflow in this turn. Could you describe \
                  what you'd like in more detail, or tell me which part to focus on?"
            .to_string(),
    }
}

/// Combines the guaranteed trail-off `fallback` question with the model's own
/// `original` text instead of discarding it (#4887 follow-up, Change 2). Even
/// after loosening `text_looks_like_question`, a future false negative must
/// never destroy the model's words — it should only ever ADD the guaranteed
/// question on top. The `fallback` is prepended (so the user sees the
/// actionable question first) and the original is kept below a divider for
/// context. When `original` is empty/whitespace-only (a genuine silent
/// turn — there's nothing to preserve), returns the fallback alone rather
/// than prepending an empty divider.
pub(super) fn combine_trail_off_fallback(fallback: &str, original: &str) -> String {
    let trimmed_original = original.trim();
    if trimmed_original.is_empty() {
        fallback.to_string()
    } else {
        format!("{fallback}\n\n---\n\n{trimmed_original}")
    }
}

/// Scans `history` in reverse for the last result from a
/// [`TRAIL_OFF_BLOCKER_TOOLS`] call that reads as a failure — a plain-text
/// error message (gate rejection), or a JSON body with `"ok": false` — and
/// returns a truncated, human-readable description of it. Tool names are
/// resolved by correlating each `ToolResults` entry's `tool_call_id` back to
/// the `AssistantToolCalls` message that issued it, so this never
/// misattributes an unrelated read-only tool's plain-text output as a
/// blocker.
fn last_builder_tool_blocker(
    history: &[crate::agent::messages::ConversationMessage],
) -> Option<String> {
    use crate::agent::messages::ConversationMessage;

    let mut call_names: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    for message in history {
        if let ConversationMessage::AssistantToolCalls { tool_calls, .. } = message {
            for call in tool_calls {
                call_names.insert(call.id.clone(), call.name.clone());
            }
        }
    }

    for message in history.iter().rev() {
        let ConversationMessage::ToolResults(results) = message else {
            continue;
        };
        for result in results.iter().rev() {
            let Some(name) = call_names.get(&result.tool_call_id) else {
                continue;
            };
            if !TRAIL_OFF_BLOCKER_TOOLS.contains(&name.as_str()) {
                continue;
            }
            // This is the MOST RECENT authoring-belt tool result in the
            // turn (results are scanned newest-first). Whatever it reads as
            // is authoritative: a success/progress result here means any
            // earlier failure from the same tool was already resolved
            // within this turn, so we must stop at this result rather than
            // keep walking backward and surfacing a stale, already-fixed
            // blocker (see review discussion on this PR).
            return describe_tool_result_blocker(&result.content)
                .map(|desc| crate::util::truncate_with_ellipsis(&desc, 500));
        }
    }
    None
}

/// Reads one builder tool result's content as a failure description, or
/// `None` when it reads as success/progress (a `workflow_proposal` payload,
/// or an `"ok": true` report). The whole body is the description, never one
/// hardcoded field, so this stays correct regardless of which fields a given
/// tool uses to explain its failure.
fn describe_tool_result_blocker(content: &str) -> Option<String> {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        if value.get("type").and_then(Value::as_str) == Some("workflow_proposal") {
            return None; // Success: a proposal was emitted.
        }
        if let Some(ok) = value.get("ok").and_then(Value::as_bool) {
            return if ok { None } else { Some(value.to_string()) };
        }
        // Some other structured payload with no `ok`/`type` marker this
        // function recognises — not confidently a blocker, skip it.
        return None;
    }
    // Non-JSON content: a hard-gate rejection (`ToolResult::error`) puts the
    // plain error message straight into the content — since every builder
    // tool's SUCCESS shape is JSON (a proposal or a `{ ok, ... }` report), a
    // bare string here is, by elimination, an error message.
    Some(trimmed.to_string())
}
