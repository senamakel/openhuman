//! The two standard web-channel turn guards — the wall-clock backstop and
//! cooperative cancellation — plus the Sentry-suppression / timeout-tagging
//! policy applied to whatever error a guarded turn produces.

use std::time::Duration;

use tokio_util::sync::CancellationToken;

use super::super::types::WebChatTaskResult;

/// Default wall-clock backstop for a single web chat turn, in seconds.
///
/// This is the OUTER safety net (issue #4746). The primary, root-cause guard is
/// the harness policy's `max_wall_clock_ms` (`tinyagents::run_policy_for`,
/// default 600s), which interrupts a hung/slow model or tool/sub-agent call
/// mid-flight and returns a proper `Timeout` → `chat_error`. This channel-level
/// backstop sits ABOVE that (900s) and only fires if a turn wedges OUTSIDE the
/// harness run entirely (e.g. session assembly / persistence plumbing), so the
/// client still always gets a terminal event instead of an empty reply / an
/// endless `inference_heartbeat` stream. Deliberately generous — a hang
/// backstop, not a UX deadline. Override via `OPENHUMAN_WEB_TURN_TIMEOUT_SECS`;
/// set it to `0` to disable the backstop.
const DEFAULT_WEB_TURN_TIMEOUT_SECS: u64 = 900;

/// Resolve the per-turn wall-clock backstop. Returns `None` when disabled
/// (env `OPENHUMAN_WEB_TURN_TIMEOUT_SECS=0`).
fn web_turn_deadline() -> Option<Duration> {
    let secs = std::env::var("OPENHUMAN_WEB_TURN_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or(DEFAULT_WEB_TURN_TIMEOUT_SECS);
    (secs > 0).then(|| Duration::from_secs(secs))
}

/// Drive a chat-turn future under the wall-clock backstop.
///
/// On elapse the inner future is dropped (cooperative teardown at its next
/// await point) and a synthetic `turn_timeout` error is returned, so the
/// caller's existing `chat_error` emission path fires. This is the outermost
/// guarantee that a wedged turn always ends in a terminal event rather than an
/// empty reply / an endless `inference_heartbeat` stream (issue #4746).
async fn drive_turn_with_deadline<F>(
    deadline: Option<Duration>,
    fut: F,
) -> Result<WebChatTaskResult, String>
where
    F: std::future::Future<Output = Result<WebChatTaskResult, String>>,
{
    match deadline {
        Some(d) => match tokio::time::timeout(d, fut).await {
            Ok(res) => res,
            Err(_elapsed) => {
                log::warn!(
                    "[web-channel] turn wall-clock backstop fired after {}s with no terminal event; \
                     emitting graceful turn_timeout chat_error (issue #4746)",
                    d.as_secs()
                );
                Err(super::super::web_errors::turn_timeout_error_message(
                    d.as_secs(),
                ))
            }
        },
        None => fut.await,
    }
}

/// Run a chat-turn future under the two standard web-channel guards, inside the
/// shared origin + approval-context scope: the cooperative cancel token
/// (interrupt/cancel paths tear the turn down at its next await point) and the
/// wall-clock backstop ([`drive_turn_with_deadline`]).
///
/// Returns `None` when the turn was cancelled cooperatively before producing a
/// result — the cancelling side already emitted the user-facing `chat_error`,
/// so the caller just unwinds quietly. Otherwise `Some(res)` carries the turn's
/// `Result`. Extracted so `start_chat` and `spawn_parallel_turn` share one copy
/// of this wiring and can't drift apart (issue #4746 review); the only per-site
/// differences are the `fork` flag and run-queue handle passed to
/// `run_chat_task` when building `fut`.
pub(crate) async fn run_turn_under_cancel_and_deadline<F>(
    cancel_token: CancellationToken,
    origin: crate::agent::turn_origin::AgentTurnOrigin,
    approval_ctx: crate::security::approval::ApprovalChatContext,
    fut: F,
) -> Option<Result<WebChatTaskResult, String>>
where
    F: std::future::Future<Output = Result<WebChatTaskResult, String>>,
{
    tokio::select! {
        biased;
        _ = cancel_token.cancelled() => None,
        res = drive_turn_with_deadline(
            web_turn_deadline(),
            crate::agent::turn_origin::with_origin(
                origin,
                crate::security::approval::APPROVAL_CHAT_CONTEXT.scope(approval_ctx, fut),
            ),
        ) => Some(res),
    }
}

/// Reason a terminal `run_chat_task` error should be kept OUT of Sentry, or
/// `None` when it is a genuine defect that must page.
///
/// A suppressed case is a deterministic, user-surfaced, retryable agent-loop
/// outcome — a terminal `chat_error` already reaches the client, so a Sentry
/// event is pure noise (same tier as `MaxIterationsExceeded` /
/// `EmptyProviderResponse`, which are demoted the same way):
///
/// - the max-iteration cap (`is_max_iterations_error`), and
/// - the **outer** web-turn wall-clock backstop (`is_outer_backstop_timeout`,
///   issue #4746) — the turn wedged outside the harness and produced no
///   terminal event, so without this arm every such turn would emit a spurious
///   Sentry event, contradicting the graceful `turn_timeout` framing.
///
/// **Not suppressed: the harness's own `Timeout` (#5804).** This arm used to
/// cover both, via `is_turn_timeout_error`, because the two are hard to tell
/// apart once stringified. They are not the same event. The outer backstop
/// fires with *nothing in flight*; the harness `Timeout` fires while bounding
/// a real model or tool call, which means the run spent its budget doing work
/// — and every result that work produced is discarded along with the turn. A
/// turn that lost eighteen sub-agents' worth of accumulated work was reported
/// here as `suppressed Sentry emission for turn wall-clock backstop` and
/// reached telemetry as nothing at all, which is why the defect survived. See
/// [`is_outer_backstop_timeout`](super::super::web_errors::is_outer_backstop_timeout)
/// for the structural argument.
///
/// The user-facing classification is deliberately untouched: both still render
/// the graceful `turn_timeout` copy via `is_turn_timeout_error`. Only the
/// telemetry decision splits.
///
/// Kept as a pure predicate over the already-formatted error string so the
/// suppression policy is unit-testable without a Sentry harness.
pub(crate) fn sentry_suppression_reason(detailed: &str) -> Option<&'static str> {
    if crate::agent::error::is_max_iterations_error(detailed) {
        Some("max-iteration cap")
    } else if super::super::web_errors::is_outer_backstop_timeout(detailed) {
        Some("turn wall-clock backstop (no terminal event)")
    } else {
        None
    }
}

/// Which wall-clock bound a reported timeout hit, as a Sentry tag value.
///
/// Only meaningful once [`sentry_suppression_reason`] has decided to report —
/// i.e. for a harness `Timeout`, never for the suppressed outer backstop. The
/// crate names the bound in the message (`RUN_BOUND_LABEL` vs
/// `PER_CALL_BOUND_LABEL`), and the two are different triage paths: a run that
/// spent its whole budget doing real work is a capacity/planning problem, while
/// one call that blew a per-call ceiling is a wedged provider. Emitting them
/// under one tag would rebuild, in the dashboard, exactly the conflation this
/// change removed from the code (#5804).
///
/// Pure over the formatted error string, for the same reason its neighbour is.
pub(crate) fn timeout_bound_tag(detailed: &str) -> &'static str {
    if detailed.contains("per-model-call ceiling") {
        "per_model_call"
    } else if detailed.contains("remaining wall-clock budget") {
        "run_remaining"
    } else if super::super::web_errors::is_turn_timeout_error(detailed) {
        "unclassified_timeout"
    } else {
        "none"
    }
}
