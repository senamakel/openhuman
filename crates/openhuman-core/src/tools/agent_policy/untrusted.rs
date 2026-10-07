//! Untrusted remote input on a read-only session: refuse acting tools at once.
//!
//! An [`AgentTurnOrigin::ExternalChannel`] turn carries text anyone on that
//! channel could have written, so prompt injection is assumed. When such a
//! turn runs on a session whose permission ceiling is read-only, an
//! external-effect tool can never legitimately run: no approval a human gives
//! later would make the read-only tier allow it. The approval gate would
//! still park the call and deny it on a ten-minute TTL, stalling the turn
//! while the model waits. This rule refuses it immediately instead, with an
//! error the model can read.
//!
//! The origin is handed in by the caller — the tool-policy middleware reads it
//! from the run context, `OpenHumanSessionHost::effective_tool_names` from its
//! argument — never recovered from ambient task-local state.

use crate::agent::turn_origin::AgentTurnOrigin;
use tinytools::PermissionLevel;

/// Whether a call must be refused outright: the turn is untrusted remote
/// input, the session may only read, and the call has an external effect.
pub fn refuses(
    origin: Option<&AgentTurnOrigin>,
    allowed: PermissionLevel,
    external_effect: bool,
) -> bool {
    external_effect
        && allowed <= PermissionLevel::ReadOnly
        && matches!(origin, Some(AgentTurnOrigin::ExternalChannel { .. }))
}

/// The tool error the model receives for a refused call.
pub fn refusal(tool: &str) -> String {
    format!(
        "Tool `{tool}` is refused: this turn is untrusted public input on a read-only \
         session, and `{tool}` has an external effect. It was not run and cannot be \
         approved from this conversation."
    )
}

#[cfg(test)]
#[path = "untrusted_tests.rs"]
mod tests;
