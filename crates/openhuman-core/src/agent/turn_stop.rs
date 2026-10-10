//! How a turn that "completed" was actually stopped by the harness.
//!
//! Three host-side mechanisms end a turn early while it still reaches the
//! normal completion path (`TurnCompleted` / `SubagentCompleted`), so without
//! this they read as clean completions in traces and to a delegating parent:
//!
//! * the repeated-failure **breaker** (`tinyagents::middleware::repeated_failure`)
//!   halts the run with a stop note;
//! * the **wind-down** at the turn deadline
//!   (`tinyagents::deadline_wind_down`) pauses the run before the backstop;
//! * the **iteration cap** stops a run whose last response still asked for
//!   tools.
//!
//! [`TurnStop`] carries that fact from where it is decided to the progress
//! events, the trace collector and the parent tool result. It is
//! content-free by construction: the failure class and operation are read
//! from the breaker's fixed note template and reduced to identifier
//! characters, never the quoted tool output or target.

/// Which mechanism stopped the turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnStopKind {
    /// The repeated-failure / classified-failure breaker halted the run.
    Breaker,
    /// The turn deadline's wind-down paused the run before the backstop.
    WindDown,
    /// The run reached its model-call cap with work still pending.
    IterationCap,
}

impl TurnStopKind {
    /// Stable wire / trace label.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Breaker => "breaker",
            Self::WindDown => "wind_down",
            Self::IterationCap => "iteration_cap",
        }
    }
}

/// A content-free description of how a turn was stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnStop {
    pub kind: TurnStopKind,
    /// Breaker only: the failure class (`uncertain_side_effect`,
    /// `permission`, …), or the stop-reason kind for notes without one
    /// (`repeated_failure`, `transient`, …).
    pub failure_class: Option<String>,
    /// Breaker only: the blocked operation (a tool name), when the note names
    /// one.
    pub operation: Option<String>,
}

/// The `Incomplete` reason the sub-agent runner reports for an iteration-cap
/// stop. Shared so the parent can tell a cap from a breaker note.
pub const SUBAGENT_ITERATION_CAP_REASON: &str = "reached its tool-call limit before finishing";

/// Longest identifier kept from a stop note.
const MAX_IDENT_CHARS: usize = 64;

impl TurnStop {
    /// A wind-down stop.
    pub fn wind_down() -> Self {
        Self {
            kind: TurnStopKind::WindDown,
            failure_class: None,
            operation: None,
        }
    }

    /// An iteration-cap stop.
    pub fn iteration_cap() -> Self {
        Self {
            kind: TurnStopKind::IterationCap,
            failure_class: None,
            operation: None,
        }
    }

    /// A breaker stop, with the class and operation read from its `note`.
    ///
    /// The classified template is `Stopping after N attempt(s): failure class
    /// `X` still blocks operation `Y` on `Z`.`; only `X` and `Y` are kept
    /// (the scope `Z` can be a URL or path). Other notes get the stop-reason
    /// kind as their class and no operation.
    pub fn breaker(note: &str) -> Self {
        let lead = note.split("Last error").next().unwrap_or(note);
        let failure_class = backticked_after(lead, "failure class `")
            .and_then(ident)
            .or_else(|| {
                let key =
                    crate::agent::session_host::stop_summary::StopReasonKind::from_stop_note(note)
                        .key();
                ident(key.strip_prefix("turn_stop.").unwrap_or(key))
            });
        let operation = backticked_after(lead, "still blocks operation `").and_then(ident);
        Self {
            kind: TurnStopKind::Breaker,
            failure_class,
            operation,
        }
    }

    /// How a top-level run ended, from the outcome's flags. The breaker is the
    /// explicit cause and wins; a wind-down pause can also leave the run
    /// without a final response (which reads as a cap), so it beats the cap.
    pub fn classify(breaker_halt: Option<&str>, wind_down: bool, hit_cap: bool) -> Option<Self> {
        if let Some(note) = breaker_halt {
            return Some(Self::breaker(note));
        }
        if wind_down {
            return Some(Self::wind_down());
        }
        hit_cap.then(Self::iteration_cap)
    }

    /// The stop behind a sub-agent's `Incomplete { reason }`: the runner's
    /// cap reason, else the breaker note it carries verbatim.
    pub fn from_incomplete_reason(reason: &str) -> Self {
        if reason == SUBAGENT_ITERATION_CAP_REASON {
            Self::iteration_cap()
        } else {
            Self::breaker(reason)
        }
    }

    /// Content-free one-line summary, e.g.
    /// `stopped: breaker uncertain_side_effect on web_answer_tool`.
    pub fn status_message(&self) -> String {
        let mut out = format!("stopped: {}", self.kind.as_str());
        if let Some(class) = &self.failure_class {
            out.push(' ');
            out.push_str(class);
        }
        if let Some(operation) = &self.operation {
            out.push_str(" on ");
            out.push_str(operation);
        }
        out
    }
}

/// The text between `marker` and the next backtick.
fn backticked_after<'a>(text: &'a str, marker: &str) -> Option<&'a str> {
    text.split_once(marker)
        .and_then(|(_, rest)| rest.split_once('`'))
        .map(|(value, _)| value)
}

/// `value` when it is a bounded identifier (`[A-Za-z0-9_.:-]`), else `None`,
/// so nothing but a class or tool name can reach a trace attribute.
fn ident(value: &str) -> Option<String> {
    let value = value.trim();
    let ok = !value.is_empty()
        && value.chars().count() <= MAX_IDENT_CHARS
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | ':' | '-'));
    ok.then(|| value.to_string())
}

#[cfg(test)]
#[path = "turn_stop_tests.rs"]
mod tests;
