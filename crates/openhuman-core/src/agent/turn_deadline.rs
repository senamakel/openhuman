//! Wall-clock deadline of one top-level agent turn.
//!
//! A web-chat turn runs under an outer backstop (`web_chat::ops::turn_guards`)
//! that drops the turn future when it elapses. Dropping it discards
//! everything the turn did. Production showed ~55 orchestrator turns a week
//! dying this way at exactly 900s, after 50–98 model calls, with nothing
//! delivered. They died there because the harness's own turn ceiling (3600s,
//! `tinyagents::turn_policy`) was later than the backstop, so the harness
//! never got the chance to stop the turn itself.
//!
//! This module is the single source of truth for the backstop and for the two
//! points the harness derives from it:
//!
//! * **wind-down** (`backstop - min(120s, backstop/5)`, 780s by default): the
//!   harness pauses the run at the next safe point, after a model call or a
//!   tool result. The driver then writes the grounded close from the work
//!   already in hand. The margin before the backstop pays for that close.
//! * **hard stop** (`backstop - min(60s, backstop/10)`, 840s by default): the
//!   harness `max_wall_clock_ms` is clamped to this. A single tool call that
//!   runs across the wind-down point is then interrupted by the harness's own
//!   typed `Timeout` before the backstop drops the turn.
//!
//! For every positive backstop, `wind_down < hard_stop < backstop`
//! (see `turn_deadline_tests.rs`).
//!
//! The web-turn guard scopes the deadline on a task-local
//! ([`with_turn_deadline`]). The top-level session turn
//! (`OpenHumanSessionHost::turn_with_origin`) copies it onto its explicit run
//! context. Turns with no backstop, such as other channels or the CLI, carry
//! `None` and keep their previous behaviour.

use std::future::Future;
use std::time::{Duration, Instant};

/// Default outer wall-clock backstop for one web-chat turn, in seconds.
pub const DEFAULT_TURN_BACKSTOP_SECS: u64 = 900;

/// Environment override for the backstop, in seconds; `0` disables it.
pub const TURN_BACKSTOP_ENV: &str = "OPENHUMAN_WEB_TURN_TIMEOUT_SECS";

/// Upper bound on the gap between wind-down and the backstop. It has to cover
/// the tool round already in flight plus the grounded close (one model call,
/// its verification, and at most one repair).
const WIND_DOWN_MARGIN: Duration = Duration::from_secs(120);

/// Upper bound on the gap between the harness hard stop and the backstop.
const HARD_STOP_MARGIN: Duration = Duration::from_secs(60);

/// Pure core of [`configured_backstop`]: an absent or unparseable value falls
/// back to [`DEFAULT_TURN_BACKSTOP_SECS`]. `0` disables the backstop (`None`).
pub fn parse_backstop(env_value: Option<&str>) -> Option<Duration> {
    let secs = env_value
        .and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or(DEFAULT_TURN_BACKSTOP_SECS);
    (secs > 0).then(|| Duration::from_secs(secs))
}

/// The backstop in force for this process (env [`TURN_BACKSTOP_ENV`]).
pub fn configured_backstop() -> Option<Duration> {
    parse_backstop(std::env::var(TURN_BACKSTOP_ENV).ok().as_deref())
}

/// One turn's backstop, anchored at the instant the backstop started counting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TurnDeadline {
    started: Instant,
    backstop: Duration,
}

impl TurnDeadline {
    /// A deadline whose backstop started at `started`.
    pub fn new(started: Instant, backstop: Duration) -> Self {
        Self { started, backstop }
    }

    /// A deadline whose backstop starts counting now.
    pub fn starting_now(backstop: Duration) -> Self {
        Self::new(Instant::now(), backstop)
    }

    /// The outer backstop, measured from [`Self::started`].
    pub fn backstop(&self) -> Duration {
        self.backstop
    }

    /// When the backstop started counting.
    pub fn started(&self) -> Instant {
        self.started
    }

    /// Elapsed budget after which the harness winds the turn down.
    pub fn wind_down_after(&self) -> Duration {
        self.backstop
            .saturating_sub(WIND_DOWN_MARGIN.min(self.backstop / 5))
    }

    /// Elapsed budget at which the harness stops the turn itself.
    pub fn hard_stop_after(&self) -> Duration {
        self.backstop
            .saturating_sub(HARD_STOP_MARGIN.min(self.backstop / 10))
    }

    /// Time spent since the backstop started, as of `now`.
    pub fn elapsed_at(&self, now: Instant) -> Duration {
        now.saturating_duration_since(self.started)
    }

    /// Whether the wind-down point has passed as of `now`.
    pub fn should_wind_down(&self, now: Instant) -> bool {
        self.elapsed_at(now) >= self.wind_down_after()
    }

    /// Time left until the hard stop as of `now` (zero once it has passed).
    pub fn hard_stop_remaining(&self, now: Instant) -> Duration {
        self.hard_stop_after().saturating_sub(self.elapsed_at(now))
    }

    /// Clamp a configured harness wall-clock ceiling (`None` means unbounded)
    /// so it ends at the hard stop, never later. Never returns `None`: a turn
    /// under a backstop is bounded whether or not the ceiling is configured.
    /// Floors at 1 ms because a zero ceiling would read as "no time at all".
    pub fn clamp_wall_clock_ms(&self, configured_ms: Option<u64>, now: Instant) -> Option<u64> {
        let remaining_ms = self
            .hard_stop_remaining(now)
            .as_millis()
            .min(u128::from(u64::MAX)) as u64;
        let remaining_ms = remaining_ms.max(1);
        Some(configured_ms.map_or(remaining_ms, |ms| ms.min(remaining_ms)))
    }
}

tokio::task_local! {
    static CURRENT_TURN_DEADLINE: TurnDeadline;
}

/// The deadline scoped on the current task, if any.
pub fn current() -> Option<TurnDeadline> {
    CURRENT_TURN_DEADLINE.try_with(|deadline| *deadline).ok()
}

/// Run `future` with `deadline` scoped on the task (a no-op scope for `None`).
pub async fn with_turn_deadline<F: Future>(deadline: Option<TurnDeadline>, future: F) -> F::Output {
    match deadline {
        Some(deadline) => CURRENT_TURN_DEADLINE.scope(deadline, future).await,
        None => future.await,
    }
}

#[cfg(test)]
#[path = "turn_deadline_tests.rs"]
mod tests;
