//! Cron turns opt out of transcript autoload before their first dispatch.
//!
//! The session-host adapter tests exercise the resulting `ResumeMode::Never`
//! transition in-process. This scheduler-level test pins the cron-specific
//! orchestration choice without binding a socket or making an HTTP request.

use super::super::agent_run::cron_turn_overrides;

#[test]
fn cron_turn_suppresses_transcript_autoload() {
    let overrides = cron_turn_overrides();

    assert!(overrides.suppress_transcript_autoload);
}
