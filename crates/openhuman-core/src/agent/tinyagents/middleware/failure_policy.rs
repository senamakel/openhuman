//! Which recovery class and budget a failed tool call gets
//! ([`recovery_policy_with_effect`]), split out of `repeated_failure.rs` so the breaker
//! stays readable: the ladder driver lives there, the classification here.

use super::call_effect::{call_effect, CallEffect};
use super::fetched_site::fetched_site_policy;
use tinyinference_llm::failure::is_recoverable_failure_text as is_recoverable_tool_failure;

/// Explicit recovery policy. Only recognised failures enter the classified
/// ledger; unknown prose continues through the established exact-repeat guard.
///
/// Judges the call's side effect from `tool`'s name alone; the breaker, which
/// sees the call's arguments and the registered tools, uses
/// [`recovery_policy_with_effect`]. Test-only: every production caller knows
/// the call's arguments.
#[cfg(test)]
pub(super) fn recovery_policy(
    tool: &str,
    error: &str,
    body_level_failure: bool,
) -> Option<(&'static str, usize)> {
    recovery_policy_with_effect(tool, error, body_level_failure, CallEffect::Unknown)
}

/// [`recovery_policy`] for a call whose side effect is already known
/// ([`super::call_effect::call_effect`]). `CallEffect::Unknown` falls back to
/// reading `tool`'s name.
pub(super) fn recovery_policy_with_effect(
    tool: &str,
    error: &str,
    body_level_failure: bool,
    effect: CallEffect,
) -> Option<(&'static str, usize)> {
    let effect = match effect {
        CallEffect::Unknown => call_effect(None, tool, &serde_json::Value::Null),
        known => known,
    };
    let (class, budget) = classified_recovery_policy(tool, error, body_level_failure, effect)?;
    // A connector, the hosted backend or the memory store refusing the
    // request (401/403, a missing or invalid key) says that service is not
    // available in this session. That is a tool to stop using, not a reason
    // to end the run: three web searches in one round came back `HTTP 401`
    // with no search provider configured, the `authentication` class halted
    // the turn on that first round, and a one-hour task ended after 101
    // seconds with the shell untouched. Steer the model off the tool once;
    // the ledger still halts if it insists.
    if matches!(class, "authentication" | "permission") && is_optional_service(tool) {
        return Some(("service_refused", 1));
    }
    // A path the model mistyped is a wrong call it can correct, not a missing
    // program: the classifier files `No such file or directory (os error 2)`
    // under `MissingApp`, which is right for a shell command and fatal for
    // `file_read`. One bad relative path ended a whole turn after two calls.
    // Any tool, not only the file tools: `use_skill` forwarding a screenshot
    // from a path that did not exist ("image forwarding failed: Failed to
    // resolve path …: No such file or directory") halted a one-hour task after
    // 92 seconds. A shell command's own "No such file" arrives as an exit
    // report, which never reaches this table.
    if class == "unsupported"
        && error
            .to_ascii_lowercase()
            .contains("no such file or directory")
    {
        return Some(("not_found", 1));
    }
    Some((class, budget))
}

/// Recovery budget for a tool rejecting its own arguments against its schema.
///
/// This is the most recoverable failure in the table: nothing ran, so there is
/// no side effect to reconcile, and the refusal hands the model the complete
/// expected schema (`tinyagents` `agent_loop/tools.rs`: "invalid arguments for
/// tool `X`: {detail}; expected schema: {…}"). It is a typo in one call, not a
/// broken world — unlike `not_found` or `unavailable`, which need the world to
/// change, and unlike a remote service's 400/422, which rejects a request the
/// model may have had every reason to send.
///
/// At a budget of 1 it ended terminal-bench 4.0 `vf2-speedup-networkx` at
/// 51/60 tests: the model omitted `edits[0].path` on two consecutive
/// `apply_patch` calls, while five others in the same run carried all three
/// fields — one of them 16,370 characters, seventeen times the size of the one
/// that failed. The omission was intermittent, not a size limit, so a further
/// attempt would most likely have landed. Three keeps it bounded; a call
/// repeated unchanged is still caught by the generic no-progress ladder.
pub(super) const ARGUMENT_SCHEMA_RECOVERY: usize = 3;

/// Prefix of `tinytools::render_command_failure`, the one renderer every
/// shell-family tool uses for a command that ran and did not exit 0: an
/// exit-code (or signal) line, then the program's own stdout and stderr.
pub(super) const COMMAND_EXIT_REPORT_PREFIX: &str = "Command failed (";

/// Whether `error` is a finished command's exit report rather than a failure
/// of the tool itself (a timeout, a policy refusal, a runtime that could not
/// be resolved), which the tools word differently.
pub(super) fn is_command_exit_report(error: &str) -> bool {
    error.trim_start().starts_with(COMMAND_EXIT_REPORT_PREFIX)
}

/// Tools that reach a service on the user's behalf: an external connector,
/// the hosted backend, or the memory store. Work can go on without them.
pub(super) fn is_optional_service(tool: &str) -> bool {
    use crate::core::all::DomainGroup;
    matches!(
        crate::tools::ops::tool_group(tool),
        DomainGroup::Integrations | DomainGroup::Hosted
    )
}

/// Read tools whose timeout is always transient, kept as a floor under the
/// side-effect reading for a call judged by name alone (no registered tool
/// facts, e.g. a dispatcher's unregistered target): a `web_fetch` declaring
/// network access must not become an uncertain action because its policy
/// does not say `read_only`, and `web_answer_tool` carries no reading verb.
/// Every entry is a production tool name; `web_search` was dropped when the
/// search tools became TinySearch's `web_*_tool` set.
const ALWAYS_RETRYABLE_ON_TIMEOUT: &[&str] = &[
    "web_search_tool",
    "web_answer_tool",
    "web_contents_tool",
    "web_fetch",
    "file_read",
    "list_files",
    "desktop_list_windows",
];

/// Whether `error` reports a failure from before the tool did anything: its
/// runtime could not be resolved or built, or its process never spawned.
/// Nothing ran, so there is nothing to reconcile, whatever the tool does.
/// Conservative on purpose: only the tools' own pre-execution wording.
pub(super) fn is_pre_execution_failure(error: &str) -> bool {
    let lower = error.to_ascii_lowercase();
    [
        "runtime unavailable",
        "runtime resolution",
        "failed to build runtime command",
        "failed to spawn",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
}

/// Whether `error` is the tool rejecting its arguments before running.
fn is_argument_rejection(error: &str) -> bool {
    let lower = error.to_ascii_lowercase();
    lower.contains("schema validation") || lower.contains("invalid arguments")
}

/// The shell agent commands run under on this host, for a nudge that has to
/// say which commands exist.
fn host_shell_description() -> String {
    let shell = if cfg!(windows) {
        "cmd.exe"
    } else {
        crate::agent::platform_shell::bash_path().unwrap_or("sh")
    };
    format!("{} (`{shell}`)", std::env::consts::OS)
}

/// The corrective nudge for a `shell` command that called a program this host
/// does not have, naming the host OS and shell so the model can switch to a
/// command that exists there (a POSIX tool on Windows is the usual case).
pub(super) fn missing_program_nudge(tool: &str, detail: &str) -> String {
    format!(
        "The `{tool}` command failed because a program it calls is not available on this host \
         ({detail}). Commands run on {}. Retry once with a command that exists there, or check \
         first (`where` on Windows, `command -v` elsewhere); do not resend it unchanged.",
        host_shell_description()
    )
}

pub(super) fn classified_recovery_policy(
    tool: &str,
    error: &str,
    body_level_failure: bool,
    effect: CallEffect,
) -> Option<(&'static str, usize)> {
    use crate::tools::status::ToolFailureClass as Class;
    if body_level_failure {
        return Some(("validation", 1));
    }
    // An unknown-tool answer is a wrong call the model can correct, and it
    // echoes the attempted name and every valid tool name. Keyword sniffing
    // below would read those names as the failure — `forbidden_tool` or a
    // name carrying `unauthorized` became `authentication`, a zero-retry
    // class, and halted the run on its first wrong guess.
    if error.trim_start().starts_with("unknown tool `") {
        return Some(("validation", 1));
    }
    // A command that ran and exited non-zero is reported as an exit-code line
    // followed by the program's own stdout and stderr. That output is data,
    // not a tool-layer verdict: keyword sniffing read `Update objects.md
    // (#401)` in a `git log | head` (exit 141, a harmless SIGPIPE) as a
    // credential failure, a zero-retry class, and ended the whole run on the
    // first call. The exit-code hint already steers the model, and the
    // generic no-progress ladder still bounds a command repeated unchanged.
    if is_command_exit_report(error) {
        return None;
    }
    // A delegated task (a TinyComputer browser task) that ended without
    // finishing. The call itself is not uncertain: the task reports what it
    // did and what to change, and its prose (a page's `403`, a planner's
    // `timed out`) is that report, not the call's verdict, so keyword
    // sniffing must not turn it into a zero-retry credential or uncertain
    // class. One changed attempt; a model gateway failing under the task
    // (5xx, connection) is transient like any other.
    if error
        .trim_start()
        .starts_with(crate::tools::status::TASK_FAILED_MARKER)
    {
        // Classify the headline only: the task report appended after the
        // blank line is page and planner text, not the call's verdict.
        let headline = error.split("\n\n").next().unwrap_or(error);
        let class = crate::tools::status::classify(headline, false).class;
        return Some(match class {
            Class::ServiceUnavailable | Class::ModelConnection => ("transient", 2),
            _ => ("task_failed", 1),
        });
    }
    // A module the host could not load stays unloaded until the app restarts,
    // so retrying the same tool cannot help. Steer the model off it once
    // rather than halting the run on the first call or spending a transient
    // budget on it (`restart the app to try again` read as recoverable).
    if error.contains(crate::tools::status::MODULE_FAULT_MARKER)
        && error.contains("restart the app to try again")
    {
        return Some(("unavailable", 1));
    }
    if let Some(policy) = fetched_site_policy(tool, error) {
        return policy;
    }
    // A tool-owned JSON error contract is less ambiguous than rendered prose.
    // Read only explicit status/code fields; arbitrary response data is not a
    // failure signal (this function is called only for `is_error` results).
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(error) {
        let status = value
            .get("status_code")
            .or_else(|| value.get("status"))
            .or_else(|| value.pointer("/error/status_code"))
            .and_then(serde_json::Value::as_u64);
        match status {
            Some(401) => return Some(("authentication", 0)),
            Some(403) => return Some(("permission", 0)),
            Some(400 | 422) => return Some(("validation", 1)),
            Some(429 | 500 | 502 | 503 | 504) => return Some(("transient", 2)),
            _ => {}
        }
        let code = value
            .get("code")
            .or_else(|| value.pointer("/error/code"))
            .and_then(serde_json::Value::as_str);
        match code {
            Some("PERMISSION_DENIED") => return Some(("permission", 0)),
            Some("UNAUTHENTICATED") => return Some(("authentication", 0)),
            Some("INVALID_ARGUMENT") => return Some(("validation", 1)),
            Some("WINDOW_NOT_FOUND") => return Some(("missing_window", 1)),
            Some("APP_NOT_FOUND") => return Some(("missing_app", 1)),
            Some("UNIMPLEMENTED") => return Some(("unsupported", 0)),
            Some("UNAVAILABLE" | "RESOURCE_EXHAUSTED") => return Some(("transient", 2)),
            _ => {}
        }
    }
    let class = crate::tools::status::classify(error, false).class;
    Some(match class {
        Class::MissingPermission => ("permission", 0),
        Class::BadCredentials => ("authentication", 0),
        // The policy refused this call before the tool ran, and its refusal
        // names a narrower, permitted alternative (scope the path, a read
        // instead of a write). One alternative is the point of that copy:
        // halting on the refusal itself ended production turns after one
        // attempt. Its own class, so a refusal does not share a budget with
        // the user's denial, and a second refusal on the same operation and
        // scope still stops.
        Class::BlockedByPolicy => ("blocked_by_policy", 1),
        // The user said no, or never answered. Asking again only re-prompts.
        Class::Denied | Class::ApprovalExpired => ("policy", 0),
        // A shell command calling a program this host lacks (often a POSIX
        // tool on Windows) is a call the model can correct: one retry, with a
        // nudge naming the host OS and shell. Other tools keep zero.
        Class::MissingApp if tool == "shell" => ("missing_app", 1),
        Class::Unsupported | Class::MissingApp => ("unsupported", 0),
        Class::NotFound
            if tool.contains("desktop") && error.to_ascii_lowercase().contains("window") =>
        {
            ("missing_window", 1)
        }
        Class::NotFound => ("not_found", 1),
        Class::ServiceUnavailable | Class::ModelConnection => ("transient", 2),
        // A schema rejection that merely names a `timeout_secs` field reads as
        // a timeout to the keyword classifier; it is a wrong call.
        Class::Timeout if is_argument_rejection(error) => {
            ("invalid_arguments", ARGUMENT_SCHEMA_RECOVERY)
        }
        // A timeout is uncertain only when the call could have changed
        // something. One that happened before the tool ran (runtime
        // resolution, spawn), or on a read (`use_skill` → `composio_list_tools`
        // halted turns on its first timeout), is retried like any transient
        // failure.
        Class::Timeout
            if ALWAYS_RETRYABLE_ON_TIMEOUT.contains(&tool)
                || is_pre_execution_failure(error)
                || effect == CallEffect::ReadOnly =>
        {
            ("transient", 2)
        }
        // A local command killed by the shell's own timeout is still uncertain
        // (it may have partly run), but it is inspectable: the model can check
        // the filesystem or re-run a smaller, bounded step. Halting the whole
        // turn on the first one threw away every earlier result for what is
        // usually a slow read (a `whois`/`dig` loop). It gets one recovery
        // attempt, steered by a reconcile-first nudge, and halts on a second.
        // Remote actions (`gmail_send`, payments, …) stay at zero: a retry
        // there can repeat an effect the agent cannot observe.
        Class::Timeout if tool == "shell" => ("uncertain_side_effect", 1),
        Class::Timeout => ("uncertain_side_effect", 0),
        // The harness's own schema-validation answer, classified from its
        // prefix before any keyword in the echoed schema could read as a
        // timeout or a credential failure.
        Class::InvalidArguments => ("invalid_arguments", ARGUMENT_SCHEMA_RECOVERY),
        // A finished command's exit report; `is_command_exit_report` above
        // already returns before this for the bare shape. Same reasoning.
        Class::CommandFailed => return None,
        Class::Unknown if is_recoverable_tool_failure(error) => ("transient", 2),
        // Its own class, not the `validation` bucket: the ledger keys on
        // (class, operation, scope), so pooling this with `unknown tool` and
        // `validate_workflow`'s invalid graphs would hand those a budget they
        // should not have. A wrong tool name does not become right, and a graph
        // the model cannot fix should still stop.
        Class::Unknown
            if error.to_ascii_lowercase().contains("schema validation")
                || error.to_ascii_lowercase().contains("invalid arguments") =>
        {
            ("invalid_arguments", ARGUMENT_SCHEMA_RECOVERY)
        }
        Class::Unknown => return None,
    })
}
