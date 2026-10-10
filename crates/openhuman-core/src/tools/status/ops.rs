//! Pure failure classification: raw tool error text → [`ClassifiedFailure`].
//!
//! This is deliberately a heuristic, keyword-driven mapping over the error
//! string the executor already produces (`agent_tool_exec` collapses a tool
//! outcome to a message + `success` flag). It touches no global state and does
//! no I/O, so every branch is unit-testable and stays cheap to call on the hot
//! tool-execution path.
//!
//! Precedence matters: the first matching class wins, so the checks are ordered
//! most-specific → least-specific. Anything unmatched falls through to
//! [`ToolFailureClass::Unknown`] (treated as recoverable so a later retry phase
//! can give it one bounded attempt rather than surfacing a dead end).

use super::types::{ClassifiedFailure, ToolFailureClass};

/// Classify a failed tool call into a user-facing [`ClassifiedFailure`].
///
/// * `error_text` — the raw error/output message from the tool. Matched
///   case-insensitively; never surfaced verbatim to the user.
/// * `timed_out` — set by the executor when the failure was a deadline stop
///   (the message alone is not always reliable), which short-circuits to
///   [`ToolFailureClass::Timeout`].
pub fn classify(error_text: &str, timed_out: bool) -> ClassifiedFailure {
    let class = classify_class(error_text, timed_out);
    describe(class)
}

/// The prefix the agent tool adapter puts on an error a tool returned as `Err`.
/// [`tool_execution_error`] is its only producer, so the classifier and the
/// adapter cannot drift apart.
const TOOL_EXECUTION_ERROR_PREFIX: &str = "Error executing ";

/// Render a tool's `Err` the way the agent tool adapter reports it to the model.
pub fn tool_execution_error(tool: &str, error: impl std::fmt::Display) -> String {
    format!("{TOOL_EXECUTION_ERROR_PREFIX}{tool}: {error}")
}

/// Does `text` (already lowercased) open with `marker`, either bare or right
/// behind the adapter's own [`tool_execution_error`] wrapper? Those are the only
/// places a producer puts a marker; the same characters anywhere else came from
/// text nobody vouched for (#6277).
fn opens_with_marker(text: &str, marker: &str) -> bool {
    let text = text.trim_start();
    if text.starts_with(marker) {
        return true;
    }
    let prefix = TOOL_EXECUTION_ERROR_PREFIX.to_ascii_lowercase();
    text.strip_prefix(prefix.as_str())
        .and_then(|rest| rest.split_once(": "))
        .is_some_and(|(tool, rest)| !tool.contains(char::is_whitespace) && rest.starts_with(marker))
}

/// The class-only half of [`classify`], split out so the ordering heuristics can
/// be tested independently of the copy.
fn classify_class(error_text: &str, timed_out: bool) -> ToolFailureClass {
    let text = error_text.to_lowercase();

    // -1. Structural result shapes a producer owns, ahead of every marker and
    //     keyword scan. Each carries a payload that is data, not a verdict:
    //     the harness's schema-validation answer echoes the whole parameter
    //     schema (a shell schema describing `timeout` read as "took too
    //     long"), a command's exit report carries the program's own stdout and
    //     stderr (stderr mentioning `ollama` read as "couldn't reach the AI
    //     model"), and an unknown-tool answer echoes the attempted arguments.
    //     The `timed_out` flag is not consulted here: the only live caller
    //     derives it by sniffing the same text.
    if let Some(class) = structural_class(&text) {
        tracing::debug!(
            ?class,
            "[tool_status::classify] matched structural result shape"
        );
        return class;
    }

    // 0. Structured policy markers win over *every* heuristic, including the
    //    `timed out` sniff below (#4459). Both markers are emitted upstream by
    //    the security/approval gate and survive the `Error: …` wrapping, so a
    //    marker hit is authoritative — a TTL-expiry deny reason literally
    //    contains "timed out", and must classify as an expired approval, never
    //    an execution Timeout that promises an auto-retry.
    //
    //    `POLICY_BLOCKED_MARKER` — a hard, cross-turn block: the action is
    //    refused by the user's safety/autonomy policy (BlockedByPolicy).
    if text.contains(crate::security::POLICY_BLOCKED_MARKER) {
        tracing::debug!("[tool_status::classify] matched POLICY_BLOCKED_MARKER -> BlockedByPolicy");
        return ToolFailureClass::BlockedByPolicy;
    }
    //    `POLICY_DENIED_MARKER` — a this-turn denial: the user answered "no" at
    //    the approval prompt, the prompt's channel dropped, the origin was
    //    unlabelled, or the prompt's TTL expired. All are
    //    non-retryable refusals (UserDeclined), split only by copy: a TTL
    //    expiry reads "approval expired", an explicit refusal reads "declined".
    if text.contains(crate::security::POLICY_DENIED_MARKER) {
        if contains_any(&text, &["timed out", "timeout", "expired"]) {
            tracing::debug!(
                "[tool_status::classify] matched POLICY_DENIED_MARKER + expiry phrase -> ApprovalExpired"
            );
            return ToolFailureClass::ApprovalExpired;
        }
        tracing::debug!("[tool_status::classify] matched POLICY_DENIED_MARKER -> Denied");
        return ToolFailureClass::Denied;
    }
    //    `NOT_FOUND_MARKER` / `UNSUPPORTED_MARKER` — the producer knew the call
    //    named something that is not there, or asked for something the target
    //    can never do (#6277). Ahead of every heuristic for the same reason as
    //    the policy markers: a catalog entry id or a skill description can
    //    contain any of the words sniffed below. Unlike the policy markers they
    //    count only where a producer puts them (`opens_with_marker`): a shell's
    //    stderr, an upstream body or an interpolated id that merely contains
    //    the marker text must not turn a failure permanent.
    if opens_with_marker(&text, super::types::NOT_FOUND_MARKER) {
        tracing::debug!("[tool_status::classify] matched NOT_FOUND_MARKER -> NotFound");
        return ToolFailureClass::NotFound;
    }
    if opens_with_marker(&text, super::types::UNSUPPORTED_MARKER) {
        tracing::debug!("[tool_status::classify] matched UNSUPPORTED_MARKER -> Unsupported");
        return ToolFailureClass::Unsupported;
    }

    // 1. Timeout — the executor's explicit signal wins over any text sniffing.
    if timed_out || contains_any(&text, &["timed out", "timeout", "deadline exceeded"]) {
        return ToolFailureClass::Timeout;
    }

    // 2. Blocked by policy — the OpenHuman security/autonomy gate or a forbidden
    //    path. Checked *before* credentials so the OpenHuman-specific
    //    `forbidden path` marker wins over the bare `forbidden` that a plain
    //    external 403 body carries (routed to credentials below). Reserved for
    //    OpenHuman policy phrasing only — a hard policy block is tagged upstream
    //    with `POLICY_BLOCKED_MARKER` and already short-circuited above (step 0);
    //    this heuristic only catches un-marked policy phrasing. Bare HTTP
    //    `403`/`Forbidden` is an external authz failure, not our gate.
    //    `channel allows` is the tail of the tool-policy PermissionDenied render.
    //    (The old `"policy denied"` needle was dead — no producer emits that
    //     phrasing; the deny family uses `POLICY_DENIED_MARKER`, handled above.)
    if contains_any(
        &text,
        &[
            "blocked by policy",
            "security policy",
            "channel allows",
            "not allowed by",
            "forbidden path",
            "autonomy",
        ],
    ) {
        return ToolFailureClass::BlockedByPolicy;
    }

    // 3. Bad credentials — auth-token problems and external authz failures
    //    (401/403). A bare HTTP 403/Forbidden or an `insufficient scopes` body
    //    means the connected account lacks the grant, so the user should
    //    reconnect / re-authorize — not toggle OpenHuman's Agent-access policy.
    //    Numeric codes go through `contains_code` so `401`/`403` never match
    //    inside a longer digit run (a port, byte count, or `14033`).
    if contains_any(
        &text,
        &[
            "unauthorized",
            "invalid api key",
            "invalid_api_key",
            "authentication failed",
            "invalid credentials",
            "bad credentials",
            "token expired",
            "invalid_grant",
            "not signed in",
            "sign in again",
            "forbidden",
            "insufficient authentication scopes",
            "insufficient scopes",
            "insufficient_scope",
        ],
    ) || contains_code(&text, "401")
        || contains_code(&text, "403")
    {
        return ToolFailureClass::BadCredentials;
    }

    // 4. Missing OS/tool permission — access denied at the filesystem/OS layer.
    if contains_any(
        &text,
        &[
            "permission denied",
            "os error 13",
            "eacces",
            "operation not permitted",
            "access is denied",
            "not permitted",
        ],
    ) {
        return ToolFailureClass::MissingPermission;
    }

    // 5. Missing app/command — the thing we tried to invoke isn't there.
    if contains_any(
        &text,
        &[
            "command not found",
            "not installed",
            "no such application",
            "could not find application",
            "executable not found",
            "is not recognized as",
            "no such file or directory (os error 2)",
        ],
    ) {
        return ToolFailureClass::MissingApp;
    }

    // 6. Model / provider connectivity — before generic service errors so a
    //    provider outage is named specifically.
    if contains_any(
        &text,
        &[
            "provider error",
            "could not reach the model",
            "could not reach model",
            "ollama",
            "llm provider",
            "inference failed",
            "model endpoint",
            "no route to host",
        ],
    ) {
        return ToolFailureClass::ModelConnection;
    }

    // 7. Service unavailable — generic transient upstream/network failure.
    if contains_any(
        &text,
        &[
            "connection refused",
            "econnrefused",
            "service unavailable",
            "temporarily unavailable",
            "could not connect",
            "connection reset",
            "network is unreachable",
        ],
    ) || contains_code(&text, "502")
        || contains_code(&text, "503")
        || contains_code(&text, "504")
    {
        return ToolFailureClass::ServiceUnavailable;
    }

    ToolFailureClass::Unknown
}

/// Prefix of the harness's answer to a schema-invalid call
/// (`tinyagents` `agent_loop/tools.rs`: "invalid arguments for tool `X`:
/// {detail}; expected schema: {…}").
const INVALID_ARGUMENTS_PREFIX: &str = "invalid arguments for tool `";

/// Prefix of the harness's answer to a call naming a tool the agent does not
/// have (`tinyagents` `agent_loop/unknown_tool.rs`: "unknown tool `X`
/// (arguments: {…}): …").
const UNKNOWN_TOOL_PREFIX: &str = "unknown tool `";

/// Prefix of `tinytools::render_command_failure`, the one renderer every
/// shell-family tool uses for a command that ran and did not exit 0
/// ("Command failed (exit code N…)" / "Command failed (terminated by a
/// signal…)"), lowercased.
const COMMAND_FAILED_PREFIX: &str = "command failed (";

/// Classify `text` (already lowercased) from a result shape its producer owns,
/// or `None` when it opens with none of them. Only the opening of the text is
/// read, never the echoed schema, arguments or program output behind it.
fn structural_class(text: &str) -> Option<ToolFailureClass> {
    if opens_with_marker(text, INVALID_ARGUMENTS_PREFIX) {
        return Some(ToolFailureClass::InvalidArguments);
    }
    if opens_with_marker(text, UNKNOWN_TOOL_PREFIX) {
        return Some(ToolFailureClass::NotFound);
    }
    if opens_with_marker(text, COMMAND_FAILED_PREFIX) {
        // The exit line is the renderer's own; 127 and 126 are the shell's
        // codes for a missing program and a non-executable one, which the
        // user can act on. Every other code is the program's verdict.
        let exit_line = text.trim_start().lines().next().unwrap_or_default();
        let code = exit_line
            .split_once("exit code ")
            .map(|(_, rest)| {
                rest.chars()
                    .take_while(char::is_ascii_digit)
                    .collect::<String>()
            })
            .unwrap_or_default();
        return Some(match code.as_str() {
            "127" => ToolFailureClass::MissingApp,
            "126" => ToolFailureClass::MissingPermission,
            _ => ToolFailureClass::CommandFailed,
        });
    }
    None
}

/// Attach the category + plain-language copy for a known class. Use this at the
/// call sites that already know the class for certain (e.g. the policy gate,
/// which knows a refusal is [`ToolFailureClass::BlockedByPolicy`]) rather than
/// round-tripping a synthetic message through [`classify`]. The copy is the
/// user-facing English source string; the UI localizes by class, so keep these
/// stable and jargon-free.
pub fn describe(class: ToolFailureClass) -> ClassifiedFailure {
    let category = class.category();
    let (cause_plain, next_action) = match class {
        ToolFailureClass::MissingPermission => (
            "OpenHuman doesn't have permission to do this yet.",
            "Grant the permission it needs, then try again.",
        ),
        ToolFailureClass::MissingApp => (
            "The app or program needed for this action isn't available.",
            "Install or open the app, then try again.",
        ),
        ToolFailureClass::ServiceUnavailable => (
            "A service OpenHuman needs is temporarily unavailable.",
            "OpenHuman will try again shortly — no action needed.",
        ),
        ToolFailureClass::BadCredentials => (
            "The saved sign-in details are missing or no longer valid.",
            "Sign in again or update the credentials, then try again.",
        ),
        ToolFailureClass::BlockedByPolicy => (
            "This action is blocked by your safety settings.",
            "Allow it in Settings → Agent access if you want it to run.",
        ),
        ToolFailureClass::ModelConnection => (
            "OpenHuman couldn't reach the AI model.",
            "Check your connection or model settings; OpenHuman will retry.",
        ),
        ToolFailureClass::Timeout => (
            "The action took too long and was stopped.",
            "OpenHuman will try again, or you can retry it manually.",
        ),
        ToolFailureClass::Denied => (
            "You declined this action.",
            "Nothing to do — it was not run. Ask again if you change your mind.",
        ),
        ToolFailureClass::ApprovalExpired => (
            "The approval request expired before anyone responded.",
            "Ask again to run it — OpenHuman won't retry it on its own.",
        ),
        ToolFailureClass::NotFound => (
            "What this action asked for doesn't exist or isn't available here.",
            "No action needed. The assistant can look up the right name and try again.",
        ),
        ToolFailureClass::Unsupported => (
            "OpenHuman can't do this automatically yet.",
            "Do it manually, or ask for a different option.",
        ),
        ToolFailureClass::InvalidArguments => (
            "The assistant called this action with details it doesn't accept.",
            "No action needed. The assistant can fix the request and try again.",
        ),
        ToolFailureClass::CommandFailed => (
            "The command ran but reported an error.",
            "No action needed. The assistant can read the output and adjust.",
        ),
        ToolFailureClass::Unknown => (
            "Something went wrong with this action.",
            "Try again; if it keeps failing, run diagnostics from Settings.",
        ),
    };
    ClassifiedFailure {
        class,
        category,
        cause_plain: cause_plain.to_string(),
        next_action: next_action.to_string(),
        recoverable: category.is_recoverable(),
    }
}

/// Case-insensitive: does `haystack` (already lowercased) contain any needle?
fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|n| haystack.contains(n))
}

/// Does `haystack` contain `code` (an all-ASCII-digit HTTP status like `"403"`)
/// as a standalone number — i.e. not embedded in a longer digit run? Guards the
/// numeric needles against false positives such as `403` inside `14033`, a port,
/// a byte count, or a timestamp. Matches when the char on each side of the hit
/// is a non-digit (or a string boundary).
fn contains_code(haystack: &str, code: &str) -> bool {
    let bytes = haystack.as_bytes();
    let mut from = 0;
    while let Some(rel) = haystack[from..].find(code) {
        let start = from + rel;
        let end = start + code.len();
        let left_ok = start == 0 || !bytes[start - 1].is_ascii_digit();
        let right_ok = end >= bytes.len() || !bytes[end].is_ascii_digit();
        if left_ok && right_ok {
            return true;
        }
        from = start + 1;
    }
    false
}

#[cfg(test)]
#[path = "ops_tests.rs"]
mod tests;
