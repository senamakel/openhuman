//! Host implementation of [`ToolOutcomeClassifier`] over OpenHuman's
//! `tool_status` domain.
//!
//! This is `docs/specs/plan-agents.md` Phase 4. The agent runtime knows a tool
//! call returned; it does not know whether the thing that came back is a
//! success, something to re-dispatch, or a dead end. OpenHuman already owns that
//! judgement in [`crate::openhuman::tool_status`], whose
//! [`classify`](crate::openhuman::tool_status::classify) turns raw tool error
//! text into a [`ClassifiedFailure`](crate::openhuman::tool_status::ClassifiedFailure)
//! (a [`ToolFailureClass`] plus a user-facing category). This adapter is the one
//! place that mapping is projected onto the crate's three-way
//! [`OutcomeClass`].
//!
//! # Contract mismatches resolved here
//!
//! **1. `ClassifiedFailure::recoverable` is NOT the retryability signal.**
//! The tempting one-liner is `if failure.recoverable { RetryableFailure }`. It
//! is wrong. `recoverable` means `category == FailureCategory::Recoverable`, and
//! [`ToolFailureClass::Unknown`] — *anything the heuristic could not classify* —
//! sits in that category so the UI can offer "try again" copy. The crate's
//! contract for [`OutcomeClass::RetryableFailure`] is much stronger: it is "an
//! assertion by the host that repeating is acceptable", made without knowing
//! whether the tool had side effects. An unclassified failure is precisely the
//! case where OpenHuman does *not* know that. So this adapter branches on the
//! **class**, not on `recoverable`, and only the three transient classes are
//! called retryable.
//!
//! That is not a new policy invention — it is the split OpenHuman's own steering
//! middleware already makes. `TinyAgents`' recoverable-failure headroom ladder
//! (`middleware.rs`, issue #4463 part 4) gates on exactly
//! `Timeout | ServiceUnavailable | ModelConnection` and lets everything else,
//! `Unknown` included, fall through to the hard-failure path. Mapping `Unknown`
//! to [`OutcomeClass::PermanentFailure`] keeps this seam consistent with that
//! guard and matches the crate's own documented safe default
//! (`ErrorFieldClassifier` classifies every error as permanent for the same
//! side-effect reason).
//!
//! **2. `classify` wants a `timed_out` flag the runtime does not carry.**
//! [`ToolResult`] has no "the executor stopped this at its deadline" bit, so the
//! flag is derived the same way `TinyAgentsToolStatusMiddleware` derives it —
//! sniffing `"timed out"` out of the combined failure text. Doing it identically
//! is the point: the middleware and this classifier must not disagree about
//! whether a call timed out.
//!
//! **3. Error text lives in two places.** The middleware combines
//! [`ToolResult::error`] and [`ToolResult::content`] before classifying (#4459),
//! because the policy markers and timeout phrases are emitted by the tool layer
//! into whichever of the two it had to hand. This adapter uses the same
//! combination, so a `[policy-denied]` marker is honoured wherever it landed and
//! a user refusal can never be re-dispatched.
//!
//! The adapter is stateless and pure, as the trait requires: no I/O, no
//! interior mutability, same answer for the same `(name, result)` every time.

use std::borrow::Cow;

use tinyagents::harness::host::{OutcomeClass, ToolOutcomeClassifier};
use tinyagents::harness::tool::ToolResult;

use crate::openhuman::tool_status::{classify, ToolFailureClass};

/// OpenHuman's [`ToolOutcomeClassifier`], backed by
/// [`crate::openhuman::tool_status::classify`].
///
/// Zero-sized and stateless: the classifier's whole knowledge base is the
/// keyword heuristic in the `tool_status` domain, which is itself pure. Every
/// instance behaves identically, so callers may construct one per session
/// without cost.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct OpenHumanToolOutcomeClassifier;

impl OpenHumanToolOutcomeClassifier {
    /// Creates the classifier.
    pub fn new() -> Self {
        Self
    }

    /// Projects one OpenHuman failure class onto the crate's coarse
    /// [`OutcomeClass`].
    ///
    /// Exhaustive on purpose — no `_` arm. A new [`ToolFailureClass`] must not
    /// silently inherit some neighbour's retry verdict; adding one should break
    /// this build and force the author to decide whether repeating the call is
    /// safe.
    ///
    /// * Transient upstream trouble (`Timeout`, `ServiceUnavailable`,
    ///   `ModelConnection`) → [`OutcomeClass::RetryableFailure`]. These are the
    ///   three the steering middleware already grants retry headroom to.
    /// * Everything else → [`OutcomeClass::PermanentFailure`]. Missing
    ///   permissions, a missing app, and bad credentials need a human to act, so
    ///   an identical re-dispatch just burns an iteration; `BlockedByPolicy`,
    ///   `Denied`, and `ApprovalExpired` are refusals that auto-retrying would
    ///   actively subvert (#4459); and `Unknown` is the case where OpenHuman has
    ///   no basis to promise a repeat is safe.
    fn class_of(failure: ToolFailureClass) -> OutcomeClass {
        match failure {
            ToolFailureClass::Timeout
            | ToolFailureClass::ServiceUnavailable
            | ToolFailureClass::ModelConnection => OutcomeClass::RetryableFailure,

            ToolFailureClass::MissingPermission
            | ToolFailureClass::MissingApp
            | ToolFailureClass::BadCredentials
            | ToolFailureClass::BlockedByPolicy
            | ToolFailureClass::Denied
            | ToolFailureClass::ApprovalExpired
            | ToolFailureClass::Unknown => OutcomeClass::PermanentFailure,
        }
    }

    /// Joins `error` and `content` into the text the heuristic reads.
    ///
    /// Mirrors `TinyAgentsToolStatusMiddleware::after_tool` exactly (#4459): the
    /// classifier historically read `error` while the marker/timeout sniffs read
    /// `content`, and the two disagreeing is the bug that combination fixed.
    /// Borrows wherever possible so the common single-source case allocates
    /// nothing on the hot path.
    fn failure_text<'a>(result: &'a ToolResult) -> Cow<'a, str> {
        let error = result.error.as_deref().unwrap_or("");
        if error.is_empty() {
            Cow::Borrowed(result.content.as_str())
        } else if result.content.is_empty() || result.content == error {
            Cow::Borrowed(error)
        } else {
            Cow::Owned(format!("{error}\n{}", result.content))
        }
    }
}

impl ToolOutcomeClassifier for OpenHumanToolOutcomeClassifier {
    fn classify(&self, name: &str, result: &ToolResult) -> OutcomeClass {
        // `error.is_none()` is the sole success signal, matching the middleware.
        // A tool that writes "Error: …" into `content` while leaving `error`
        // unset has reported success; second-guessing that here would make the
        // two paths disagree about whether the call failed at all.
        if result.error.is_none() {
            return OutcomeClass::Success;
        }

        let text = Self::failure_text(result);
        // No executor deadline flag reaches the runtime, so derive it the way
        // the middleware does. `classify` short-circuits the policy markers
        // ahead of this flag, so a TTL-expired approval whose reason literally
        // contains "timed out" still classifies as `ApprovalExpired`, never a
        // retryable `Timeout` (#4459).
        let timed_out = text.contains("timed out");
        let failure = classify(&text, timed_out);
        let outcome = Self::class_of(failure.class);

        tracing::debug!(
            target: "tinyagents",
            tool = %name,
            class = ?failure.class,
            ?outcome,
            "[tinyagents::host] classified tool outcome"
        );
        outcome
    }
}

// TODO(phase4): rate limits (`429`, "rate limit", "too many requests",
// "retry after") and DNS blips ("dns error", "failed to resolve") currently fall
// through `tool_status::classify` to `Unknown` and therefore land on
// `PermanentFailure` here, even though they are textbook retryable. The phrase
// list that does recognise them is `is_recoverable_tool_failure`, a private fn in
// `src/openhuman/tinyagents/middleware.rs`. Copying it into this adapter would
// fork the heuristic; the fix is to move those needles into
// `tool_status::ops::classify_class` (a `RateLimited` class, or extending
// `ServiceUnavailable`) and let both callers read one list. Not done here because
// Phase 4 must not edit existing files.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::openhuman::security::{POLICY_BLOCKED_MARKER, POLICY_DENIED_MARKER};

    fn result(error: Option<&str>, content: &str) -> ToolResult {
        ToolResult {
            call_id: "call-1".to_string(),
            name: "shell".to_string(),
            content: content.to_string(),
            raw: None,
            error: error.map(str::to_string),
            elapsed_ms: 5,
        }
    }

    fn outcome_of(error: &str) -> OutcomeClass {
        OpenHumanToolOutcomeClassifier::new().classify("shell", &result(Some(error), ""))
    }

    // ── class → OutcomeClass mapping (exhaustive) ─────────────────────────────

    #[test]
    fn every_failure_class_maps_as_documented() {
        use OutcomeClass::*;
        use ToolFailureClass::*;
        for (class, want) in [
            (Timeout, RetryableFailure),
            (ServiceUnavailable, RetryableFailure),
            (ModelConnection, RetryableFailure),
            (MissingPermission, PermanentFailure),
            (MissingApp, PermanentFailure),
            (BadCredentials, PermanentFailure),
            (ToolFailureClass::BlockedByPolicy, PermanentFailure),
            (Denied, PermanentFailure),
            (ApprovalExpired, PermanentFailure),
            (Unknown, PermanentFailure),
        ] {
            assert_eq!(
                OpenHumanToolOutcomeClassifier::class_of(class),
                want,
                "mapping {class:?}"
            );
        }
    }

    #[test]
    fn no_failure_class_ever_maps_to_success() {
        // `class_of` is only reached when `error` is set, so a mapping that
        // produced `Success` would erase a real failure from the transcript.
        for class in [
            ToolFailureClass::Timeout,
            ToolFailureClass::ServiceUnavailable,
            ToolFailureClass::ModelConnection,
            ToolFailureClass::MissingPermission,
            ToolFailureClass::MissingApp,
            ToolFailureClass::BadCredentials,
            ToolFailureClass::BlockedByPolicy,
            ToolFailureClass::Denied,
            ToolFailureClass::ApprovalExpired,
            ToolFailureClass::Unknown,
        ] {
            assert!(
                OpenHumanToolOutcomeClassifier::class_of(class).is_failure(),
                "{class:?} must stay a failure"
            );
        }
    }

    #[test]
    fn recoverable_flag_is_deliberately_not_the_retry_signal() {
        // `Unknown` is `FailureCategory::Recoverable` in the domain, yet must be
        // permanent here — this divergence is the whole point of the adapter and
        // regressing to `if failure.recoverable` would resurrect it.
        let unknown = crate::openhuman::tool_status::describe(ToolFailureClass::Unknown);
        assert!(
            unknown.recoverable,
            "domain still calls Unknown recoverable"
        );
        assert_eq!(
            OpenHumanToolOutcomeClassifier::class_of(ToolFailureClass::Unknown),
            OutcomeClass::PermanentFailure
        );
    }

    // ── end-to-end over real error text ───────────────────────────────────────

    #[test]
    fn absent_error_is_success_even_with_scary_content() {
        let classifier = OpenHumanToolOutcomeClassifier::new();
        assert_eq!(
            classifier.classify("shell", &result(None, "Error: everything is on fire")),
            OutcomeClass::Success
        );
    }

    #[test]
    fn transient_failures_are_retryable() {
        assert_eq!(
            outcome_of("tool 'http_request' timed out after 120 seconds"),
            OutcomeClass::RetryableFailure
        );
        assert_eq!(
            outcome_of("upstream returned 503 Service Unavailable"),
            OutcomeClass::RetryableFailure
        );
        assert_eq!(
            outcome_of("ollama daemon not responding"),
            OutcomeClass::RetryableFailure
        );
    }

    #[test]
    fn user_actionable_failures_are_permanent() {
        assert_eq!(
            outcome_of("Permission denied (os error 13)"),
            OutcomeClass::PermanentFailure
        );
        assert_eq!(
            outcome_of("bash: gh: command not found"),
            OutcomeClass::PermanentFailure
        );
        assert_eq!(
            outcome_of("HTTP 401 Unauthorized"),
            OutcomeClass::PermanentFailure
        );
    }

    #[test]
    fn unclassifiable_failures_are_permanent_not_retryable() {
        assert_eq!(
            outcome_of("some totally novel failure mode"),
            OutcomeClass::PermanentFailure
        );
    }

    // ── policy guards must never be re-dispatched ─────────────────────────────

    #[test]
    fn a_policy_block_is_never_retryable() {
        let text = format!("{POLICY_BLOCKED_MARKER} destructive command refused");
        assert_eq!(outcome_of(&text), OutcomeClass::PermanentFailure);
    }

    #[test]
    fn a_user_denial_is_never_retryable() {
        let text = format!("{POLICY_DENIED_MARKER} you declined this shell action");
        assert!(!outcome_of(&text).is_retryable());
    }

    #[test]
    fn an_expired_approval_is_permanent_despite_saying_timed_out() {
        // The single most dangerous mis-mapping: a TTL-expiry deny reason
        // literally contains "timed out", and reading it as a retryable Timeout
        // would auto-re-run an effect nobody approved (#4459).
        let text = format!("{POLICY_DENIED_MARKER} Approval for 'shell' timed out after 600s");
        assert_eq!(outcome_of(&text), OutcomeClass::PermanentFailure);
    }

    // ── failure-text assembly ─────────────────────────────────────────────────

    #[test]
    fn markers_are_honoured_when_they_land_in_content_not_error() {
        // The tool layer puts the marker in whichever field it had to hand; the
        // combined sniff is what makes both placements behave the same.
        let denied = format!("{POLICY_DENIED_MARKER} declined");
        let classifier = OpenHumanToolOutcomeClassifier::new();
        assert_eq!(
            classifier.classify("shell", &result(Some("tool failed"), &denied)),
            OutcomeClass::PermanentFailure
        );
    }

    #[test]
    fn failure_text_borrows_when_one_side_is_empty_or_duplicated() {
        let only_error = result(Some("boom"), "");
        assert!(matches!(
            OpenHumanToolOutcomeClassifier::failure_text(&only_error),
            Cow::Borrowed("boom")
        ));

        let duplicated = result(Some("boom"), "boom");
        assert!(matches!(
            OpenHumanToolOutcomeClassifier::failure_text(&duplicated),
            Cow::Borrowed("boom")
        ));

        let only_content = result(Some(""), "boom");
        assert!(matches!(
            OpenHumanToolOutcomeClassifier::failure_text(&only_content),
            Cow::Borrowed("boom")
        ));

        let both = result(Some("boom"), "context");
        assert_eq!(
            OpenHumanToolOutcomeClassifier::failure_text(&both),
            "boom\ncontext"
        );
    }

    #[test]
    fn an_error_present_but_empty_is_still_a_failure() {
        // `Some("")` is a tool reporting failure without a message. Matching the
        // crate's baseline classifier, the field's presence is the signal.
        let classifier = OpenHumanToolOutcomeClassifier::new();
        assert_eq!(
            classifier.classify("shell", &result(Some(""), "")),
            OutcomeClass::PermanentFailure
        );
    }

    // ── trait-level invariants ────────────────────────────────────────────────

    #[test]
    fn classification_is_pure_and_repeatable() {
        let classifier = OpenHumanToolOutcomeClassifier::new();
        let r = result(Some("connection refused"), "");
        let first = classifier.classify("http_request", &r);
        assert_eq!(first, classifier.classify("http_request", &r));
        assert_eq!(first, OutcomeClass::RetryableFailure);
    }

    #[test]
    fn the_dispatched_name_does_not_change_the_verdict_today() {
        // Documented, not aspirational: OpenHuman's taxonomy is text-driven, so
        // `name` is used only for logging. If per-tool overrides are ever added
        // (a 404 meaning "no such record" for one integration and a dead end for
        // another), this test is the one that must change.
        let classifier = OpenHumanToolOutcomeClassifier::new();
        let r = result(Some("HTTP 403 Forbidden"), "");
        assert_eq!(
            classifier.classify("gmail_send", &r),
            classifier.classify("shell", &r)
        );
    }

    #[test]
    fn usable_as_a_trait_object() {
        let classifier: std::sync::Arc<dyn ToolOutcomeClassifier> =
            std::sync::Arc::new(OpenHumanToolOutcomeClassifier::default());
        assert_eq!(
            classifier.classify("shell", &result(Some("503"), "")),
            OutcomeClass::RetryableFailure
        );
    }
}
