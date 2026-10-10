//! Support types for the journal projection: collector actions that are not
//! [`AgentProgress`] events, and the pure text/outcome extraction helpers.

use tinyagents_harness::terminal::{TerminalClass, TerminalOutcome};

use crate::agent::progress::AgentProgress;
use crate::agent::progress_tracing::TurnOutcome;

/// One step applied to the [`super::SpanCollector`] while replaying a journal.
///
/// Most observations map to [`AgentProgress`] stamped at the observation's
/// own time; a few carry facts `AgentProgress` cannot express (a model call's
/// real request start, the turn's failure) or need a different timestamp.
#[derive(Debug)]
pub(super) enum Replay {
    /// A progress event stamped at an explicit time (not the observation's).
    At(AgentProgress, u64),
    /// The next model call in this scope (`None` = top level) started here.
    CallStart(Option<String>, u64),
    /// The top-level run ended this way.
    Outcome(TurnOutcome),
}

/// Map a top-level `RunFailed` onto the turn outcome its span reports.
pub(super) fn turn_outcome_for_failure(
    error: &str,
    outcome: Option<&TerminalOutcome>,
) -> TurnOutcome {
    let message = outcome
        .map(|o| o.message.clone())
        .filter(|m| !m.trim().is_empty())
        .unwrap_or_else(|| error.to_string());
    match outcome.map(|o| o.class) {
        Some(TerminalClass::Cancellation) => TurnOutcome::Cancelled {
            reason: Some(message),
        },
        Some(TerminalClass::Timeout) => TurnOutcome::TimedOut { message },
        _ => TurnOutcome::Failed { message },
    }
}

/// Text of a captured message payload: a bare string, an object's `content`
/// string, else its JSON.
pub(super) fn json_content_text(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(text) => text.clone(),
        serde_json::Value::Object(map) => map
            .get("content")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| value.to_string()),
        _ => value.to_string(),
    }
}

/// The user's message from a captured model request: the text of the last
/// user-role message in the request array. Handles both the role-tagged shape
/// (`{"role": "user", "content": …}`) and the crate's externally tagged
/// `Message` (`{"user": {"content": [{"text": …}]}}`). Falls back to
/// [`json_content_text`] when the payload holds no user message.
pub(super) fn user_message_text(value: &serde_json::Value) -> String {
    let user_payload = value.as_array().and_then(|messages| {
        messages.iter().rev().find_map(|message| {
            let object = message.as_object()?;
            if object.get("role").and_then(serde_json::Value::as_str) == Some("user") {
                return Some(message);
            }
            object.get("user")
        })
    });
    match user_payload {
        Some(payload) => content_text(payload.get("content").unwrap_or(payload)),
        None => json_content_text(value),
    }
}

/// Visible text of a `content` value: a string, or the text blocks of a
/// content-block array joined by newlines.
fn content_text(content: &serde_json::Value) -> String {
    match content {
        serde_json::Value::String(text) => text.clone(),
        serde_json::Value::Array(blocks) => blocks
            .iter()
            .filter_map(|block| match block {
                serde_json::Value::String(text) => Some(text.clone()),
                serde_json::Value::Object(object) => object
                    .get("text")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n"),
        other => other.to_string(),
    }
}

#[cfg(test)]
#[path = "journal_replay_tests.rs"]
mod tests;
