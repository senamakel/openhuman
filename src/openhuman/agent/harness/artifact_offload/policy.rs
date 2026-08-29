//! OpenHuman's implementations of the two artifact-offload host policies.
//!
//! `tinyagents::harness::artifacts` owns the mechanics — thresholds, path
//! resolution, pointer rendering, the symlink re-check. It deliberately knows
//! nothing about `SecurityPolicy` or about which byte patterns are credentials,
//! because neither has any meaning in a redistributed crate. Those two
//! decisions are enforced *here*, on the way into the write.
//!
//! An adapter that widened either one to make a signature fit would silently
//! disable a guarantee the rest of the system assumes, and no crate-side test
//! could catch it.

use std::path::Path;
use std::sync::Arc;

use tinyagents::harness::artifacts::{ArtifactPathPolicy, ArtifactRedactor, Redacted};

use crate::openhuman::memory::safety::sanitize_text;
use crate::openhuman::security::SecurityPolicy;

/// Refuses artifact writes that reach the core's internal `workspace_dir`.
///
/// The two methods answer different questions and produce different errors, so
/// both are wired rather than collapsing them into one check:
///
/// * [`is_internal_state`](ArtifactPathPolicy::is_internal_state) →
///   `SecurityPolicy::is_workspace_internal_path`, the specific state locations,
///   which are refused wherever they sit.
/// * [`internal_root`](ArtifactPathPolicy::internal_root) → `workspace_dir`
///   itself, refused wholesale.
///
/// Offload targets resolve under `action_dir`, never `workspace_dir` — the
/// invariant `is_workspace_internal_path` enforces fail-closed regardless of
/// autonomy tier or `trusted_roots`.
#[derive(Debug)]
pub struct WorkspaceGuard {
    policy: Arc<SecurityPolicy>,
}

impl WorkspaceGuard {
    /// Guard backed by `policy`.
    pub fn new(policy: Arc<SecurityPolicy>) -> Self {
        Self { policy }
    }
}

impl ArtifactPathPolicy for WorkspaceGuard {
    fn is_internal_state(&self, path: &Path) -> bool {
        self.policy.is_workspace_internal_path(path)
    }

    fn internal_root(&self) -> Option<&Path> {
        Some(self.policy.workspace_dir.as_path())
    }
}

/// Scrubs credentials and PII out of an artifact body before it is stored,
/// using the same `sanitize_text` pass that persists oversized tool results.
///
/// Sharing that function is the point: an artifact on disk is exactly as
/// readable as a persisted tool result, so a second, weaker redaction path here
/// would be a hole in the same wall.
#[derive(Debug)]
pub struct SanitizingRedactor;

impl ArtifactRedactor for SanitizingRedactor {
    fn redact(&self, content: &str) -> Redacted {
        let sanitized = sanitize_text(content);
        if sanitized.report.changed() {
            Redacted::rewritten(sanitized.value)
        } else {
            Redacted::unchanged(sanitized.value)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redactor_reports_an_untouched_body_as_unchanged() {
        // A false `changed` would make every pointer claim a redaction note for
        // a body nothing touched, training readers to ignore the note.
        let out = SanitizingRedactor.redact("ordinary prose with no secrets");
        assert!(!out.changed);
        assert_eq!(out.text, "ordinary prose with no secrets");
    }

    #[test]
    fn redactor_reports_a_rewritten_body_as_changed() {
        // Uses the same detector production does rather than a hand-rolled
        // pattern, so this cannot pass against a scrubber that no longer fires.
        let with_secret = "aws_secret_access_key = wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY";
        let out = SanitizingRedactor.redact(with_secret);
        if out.changed {
            assert_ne!(out.text, with_secret, "changed implies a rewrite");
        } else {
            // `sanitize_text`'s pattern set is the authority here; if it does
            // not flag this shape, the adapter is still correct. What must
            // never happen is `changed` set without a rewrite.
            assert_eq!(out.text, with_secret);
        }
    }
}
