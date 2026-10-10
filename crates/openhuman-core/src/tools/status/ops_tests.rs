use super::*;

fn class_of(text: &str) -> ToolFailureClass {
    classify(text, false).class
}

#[test]
fn timeout_flag_wins_regardless_of_text() {
    assert_eq!(
        classify("anything at all", true).class,
        ToolFailureClass::Timeout
    );
}

#[test]
fn timeout_detected_from_text() {
    assert_eq!(
        class_of("tool 'shell' timed out after 120 seconds"),
        ToolFailureClass::Timeout
    );
}

#[test]
fn missing_permission_from_os_error() {
    assert_eq!(
        class_of("Error executing file_write: Permission denied (os error 13)"),
        ToolFailureClass::MissingPermission
    );
    assert_eq!(
        class_of("EACCES: operation not permitted"),
        ToolFailureClass::MissingPermission
    );
}

#[test]
fn missing_app_from_command_not_found() {
    assert_eq!(
        class_of("bash: gh: command not found"),
        ToolFailureClass::MissingApp
    );
    assert_eq!(
        class_of("ffmpeg is not installed on this system"),
        ToolFailureClass::MissingApp
    );
}

#[test]
fn service_unavailable_from_connection_errors() {
    assert_eq!(
        class_of("connection refused (ECONNREFUSED)"),
        ToolFailureClass::ServiceUnavailable
    );
    assert_eq!(
        class_of("upstream returned 503 Service Unavailable"),
        ToolFailureClass::ServiceUnavailable
    );
}

#[test]
fn bad_credentials_from_auth_errors() {
    assert_eq!(
        class_of("HTTP 401 Unauthorized"),
        ToolFailureClass::BadCredentials
    );
    assert_eq!(
        class_of("invalid api key provided"),
        ToolFailureClass::BadCredentials
    );
    assert_eq!(
        class_of("auth token expired, please sign in again"),
        ToolFailureClass::BadCredentials
    );
}

#[test]
fn blocked_by_policy_from_gate_and_forbidden() {
    assert_eq!(
        class_of("Permission denied for tool 'shell': requires Execute, channel allows ReadOnly"),
        ToolFailureClass::BlockedByPolicy
    );
    assert_eq!(
        class_of("blocked by policy: destructive command"),
        ToolFailureClass::BlockedByPolicy
    );
    // OpenHuman's own path guard stays policy...
    assert_eq!(
        class_of("write rejected: forbidden path outside action_dir"),
        ToolFailureClass::BlockedByPolicy
    );
}

#[test]
fn external_403_is_credentials_not_policy() {
    // A bare external authz failure must route to credentials (reconnect /
    // grant scopes), NOT OpenHuman's Agent-access policy.
    assert_eq!(
        class_of("HTTP 403 Forbidden"),
        ToolFailureClass::BadCredentials
    );
    assert_eq!(
        class_of("Gmail API error: 403 insufficient authentication scopes"),
        ToolFailureClass::BadCredentials
    );
    assert_eq!(
        class_of("401 Unauthorized"),
        ToolFailureClass::BadCredentials
    );
}

#[test]
fn policy_denied_marker_is_denied_and_not_recoverable() {
    use crate::security::POLICY_DENIED_MARKER;
    let text = format!("{POLICY_DENIED_MARKER} you declined this shell action");
    assert_eq!(class_of(&text), ToolFailureClass::Denied);
    // UserDeclined family — never eligible for an auto-retry (#4459).
    assert!(!classify(&text, false).recoverable);
}

#[test]
fn policy_denied_ttl_expiry_is_approval_expired_not_timeout() {
    use crate::security::POLICY_DENIED_MARKER;
    // A TTL-expiry deny reason literally contains "timed out" — the policy
    // marker must win over the timeout sniff so it classifies as an expired
    // approval, NOT an execution Timeout that promises an auto-retry (#4459).
    let text = format!("{POLICY_DENIED_MARKER} Approval for 'shell' timed out after 600s");
    assert_eq!(class_of(&text), ToolFailureClass::ApprovalExpired);
    assert_ne!(class_of(&text), ToolFailureClass::Timeout);
    assert!(!classify(&text, false).recoverable);
}

#[test]
fn numeric_status_codes_need_word_boundaries() {
    // `403`/`503` embedded in a longer digit run must NOT trip the code
    // needles — these fall through to Unknown.
    assert_eq!(
        class_of("processed 14033 records before aborting"),
        ToolFailureClass::Unknown
    );
    assert_eq!(
        class_of("listening on port 15032 failed unexpectedly"),
        ToolFailureClass::Unknown
    );
    // ...but a standalone 503 is still a service outage.
    assert_eq!(
        class_of("upstream returned 503"),
        ToolFailureClass::ServiceUnavailable
    );
}

#[test]
fn model_connection_from_provider_errors() {
    assert_eq!(
        class_of("Provider error (retryable=true): boom"),
        ToolFailureClass::ModelConnection
    );
    assert_eq!(
        class_of("could not reach the model endpoint"),
        ToolFailureClass::ModelConnection
    );
    assert_eq!(
        class_of("ollama daemon not responding"),
        ToolFailureClass::ModelConnection
    );
}

#[test]
fn unknown_when_nothing_matches() {
    assert_eq!(
        class_of("some totally novel failure mode"),
        ToolFailureClass::Unknown
    );
}

#[test]
fn credentials_precedence_over_service_when_both_present() {
    // A 401 that also mentions a connection should read as credentials, not
    // a transient service blip — the ordering guarantees this.
    assert_eq!(
        class_of("could not connect: 401 unauthorized"),
        ToolFailureClass::BadCredentials
    );
}

#[test]
fn every_class_produces_nonempty_user_copy() {
    for class in [
        ToolFailureClass::MissingPermission,
        ToolFailureClass::MissingApp,
        ToolFailureClass::ServiceUnavailable,
        ToolFailureClass::BadCredentials,
        ToolFailureClass::BlockedByPolicy,
        ToolFailureClass::ModelConnection,
        ToolFailureClass::Timeout,
        ToolFailureClass::Denied,
        ToolFailureClass::ApprovalExpired,
        ToolFailureClass::NotFound,
        ToolFailureClass::Unsupported,
        ToolFailureClass::Unknown,
    ] {
        let f = describe(class);
        assert!(!f.cause_plain.is_empty(), "empty cause for {class:?}");
        assert!(!f.next_action.is_empty(), "empty next_action for {class:?}");
        assert_eq!(f.recoverable, f.category.is_recoverable());
    }
}

// #6277: failures whose producer knows they cannot succeed on retry are
// classified from its marker, ahead of any word the message happens to contain.
#[test]
fn marked_permanent_failures_are_not_recoverable() {
    use crate::tools::status::{NOT_FOUND_MARKER, UNSUPPORTED_MARKER};

    // Through the adapter's own wrapper, exactly as a tool's `Err` reaches the
    // classifier.
    let not_found = classify(
        &tool_execution_error(
            "use_skill",
            format!("{NOT_FOUND_MARKER} skill 'timeout-helper' not found"),
        ),
        false,
    );
    assert_eq!(not_found.class, ToolFailureClass::NotFound);
    assert!(!not_found.recoverable);

    let unsupported = classify(
        &format!("{UNSUPPORTED_MARKER} Failed to install skill 'x': hosted on skills.sh"),
        false,
    );
    assert_eq!(unsupported.class, ToolFailureClass::Unsupported);
    assert!(!unsupported.recoverable);
}

// #6277 review: a marker counts only where a producer puts it. Text that merely
// contains one (shell stderr, an upstream body, an interpolated id) keeps its
// ordinary classification.
#[test]
fn markers_outside_the_producer_prefix_do_not_count() {
    use crate::tools::status::{NOT_FOUND_MARKER, UNSUPPORTED_MARKER};

    for text in [
        format!("grep: pattern {NOT_FOUND_MARKER} not in log"),
        format!("upstream said: {UNSUPPORTED_MARKER}"),
        format!("Failed to install skill '{NOT_FOUND_MARKER}x': network hiccup"),
        tool_execution_error("shell", format!("exit 1\nstderr: {UNSUPPORTED_MARKER}")),
    ] {
        let class = classify(&text, false).class;
        assert!(
            !matches!(
                class,
                ToolFailureClass::NotFound | ToolFailureClass::Unsupported
            ),
            "{text:?} carries a marker outside the producer prefix but classified {class:?}"
        );
    }
}

#[test]
fn recoverable_flag_matches_category() {
    assert!(classify("503 service unavailable", false).recoverable);
    assert!(!classify("permission denied (os error 13)", false).recoverable);
    assert!(!classify("blocked by policy", false).recoverable);
}

// Production misfires (Langfuse, Oct 2026): the classifier keyword-scanned the
// whole result, including the JSON schema the harness echoes back on a
// validation error and a command's own stderr. Structural shapes a producer
// owns are classified before any keyword scan, and their payload is never
// read for provider / timeout / credential words.
#[test]
fn structural_results_classify_before_any_keyword_scan() {
    // The harness's schema-validation answer (`agent_loop/tools.rs`). The
    // echoed shell schema describes a `timeout` argument; that must not read
    // as "the action took too long".
    let invalid_shell_args = "invalid arguments for tool `shell`: validation error: tool `shell` \
         arguments failed schema validation: missing required property `command`; expected \
         schema: {\"type\":\"object\",\"properties\":{\"command\":{\"type\":\"string\"},\
         \"timeout\":{\"type\":\"integer\",\"description\":\"Timeout in seconds; the command \
         is killed after the deadline (timed out)\"}},\"required\":[\"command\"]}";
    // A command that ran and exited non-zero, rendered by
    // `tinytools::render_command_failure`. Its stderr mentions ollama; that is
    // the program's output, not OpenHuman failing to reach a model.
    let ollama_stderr = "Command failed (exit code 1)\n[stdout]\nchecking models\n[stderr]\n\
         Error: could not connect to ollama at 127.0.0.1:11434 (connection refused)";

    let cases: &[(&str, &str, bool, ToolFailureClass)] = &[
        (
            "empty-args shell validation error with `timeout` in the echoed schema",
            invalid_shell_args,
            false,
            ToolFailureClass::InvalidArguments,
        ),
        (
            "validation error even when the executor's text sniff set timed_out",
            invalid_shell_args,
            true,
            ToolFailureClass::InvalidArguments,
        ),
        (
            "validation error carrying credential/policy words in the schema",
            "invalid arguments for tool `gmail_send`: missing `to`; expected schema: \
             {\"description\":\"401 unauthorized forbidden; blocked by policy; permission denied\"}",
            false,
            ToolFailureClass::InvalidArguments,
        ),
        (
            "non-zero exit whose stderr mentions ollama",
            ollama_stderr,
            false,
            ToolFailureClass::CommandFailed,
        ),
        (
            "non-zero exit whose stderr says timed out (text sniff set timed_out)",
            "Command failed (exit code 1)\n[stderr]\npytest: test_fetch timed out after 5s",
            true,
            ToolFailureClass::CommandFailed,
        ),
        (
            "non-zero exit whose output carries a 401 / unauthorized",
            "Command failed (exit code 22)\n[stderr]\ncurl: (22) The requested URL returned \
             error: 401 Unauthorized",
            false,
            ToolFailureClass::CommandFailed,
        ),
        (
            "signal-terminated command",
            "Command failed (terminated by a signal — no exit code)\n(no output was captured on \
             stdout or stderr)",
            false,
            ToolFailureClass::CommandFailed,
        ),
        (
            "SIGPIPE exit whose hint is pure data",
            "Command failed (exit code 141 — SIGPIPE: a reader closed the pipe before the writer \
             finished)\n[stdout]\nUpdate objects.md (#401)",
            false,
            ToolFailureClass::CommandFailed,
        ),
        (
            "exit 127 keeps its missing-program meaning from the exit line",
            "Command failed (exit code 127 — command not found: a required executable or \
             dependency is missing or not on PATH)\n[stderr]\nbash: ollama: command not found",
            false,
            ToolFailureClass::MissingApp,
        ),
        (
            "exit 126 keeps its permission meaning from the exit line",
            "Command failed (exit code 126 — permission denied or not executable)\n[stderr]\n\
             bash: ./run.sh: Permission denied",
            false,
            ToolFailureClass::MissingPermission,
        ),
        (
            "exit report wrapped by the agent tool adapter",
            "Error executing shell: Command failed (exit code 2)\n[stderr]\nprovider error: \
             service unavailable",
            false,
            ToolFailureClass::CommandFailed,
        ),
        (
            "unknown-tool answer echoing arguments that contain keywords",
            "unknown tool `fetch_url` (arguments: {\"url\":\"https://x.test\",\"timeout\":30,\
             \"note\":\"ollama 401\"}): no tool with that name is available to you, and calling \
             it again will fail the same way.",
            false,
            ToolFailureClass::NotFound,
        ),
    ];
    for (name, text, timed_out, expected) in cases {
        assert_eq!(
            classify(text, *timed_out).class,
            *expected,
            "case `{name}` misclassified: {text:?}"
        );
    }
}

// The structural prefixes count only at the start, like the producer markers:
// prose that merely mentions them keeps its ordinary classification.
#[test]
fn structural_prefixes_only_count_where_the_producer_puts_them() {
    assert_eq!(
        class_of("request timed out; Command failed (exit code 1) was logged earlier"),
        ToolFailureClass::Timeout
    );
    assert_eq!(
        class_of("ollama: provider error, see invalid arguments for tool `x` in the log"),
        ToolFailureClass::ModelConnection
    );
}

#[test]
fn structural_classes_carry_their_own_copy() {
    let invalid = classify(
        "invalid arguments for tool `shell`: missing `command`",
        false,
    );
    assert_eq!(invalid.class, ToolFailureClass::InvalidArguments);
    assert!(!invalid.recoverable);
    assert!(!invalid.cause_plain.contains("took too long"));

    let exit = classify("Command failed (exit code 1)\n[stderr]\nboom", false);
    assert_eq!(exit.class, ToolFailureClass::CommandFailed);
    assert!(!exit.recoverable);
    assert!(!exit.cause_plain.contains("AI model"));
}
