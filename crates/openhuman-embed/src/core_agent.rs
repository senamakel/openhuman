//! The agent facade of a caller-built runtime.
//!
//! [`CoreAgent`] is what [`Core::agent`](crate::Core::agent) hands out: a
//! borrowed newtype over the runtime whose turns run the **orchestrator** on
//! the runtime's own config, dispatched as the `inference.agent_chat` RPC.
//! It is the right shape for a host that already owns a
//! [`CoreRuntime`](openhuman_core::core::runtime::CoreRuntime) — the desktop
//! shell, an existing embedder — and wants a typed turn on it.
//!
//! A host that wants *its own* agents — several, each with its own MCP
//! servers, skills, working directory and access tier — wants
//! [`Runtime::agent`](crate::Runtime::agent) and the [`Agent`](crate::Agent)
//! handle instead, whose turns dispatch natively under a per-agent context.

use std::sync::Arc;

use crate::error::CoreError;
use crate::turn::{Turn, TurnOutcome, TurnTarget};
use openhuman_core::core::runtime::CoreRuntime;

/// Typed access to the agent harness of a caller-built runtime.
///
/// Obtained from [`Core::agent`](crate::Core::agent); never constructed
/// directly.
pub struct CoreAgent<'a>(pub(crate) &'a Arc<CoreRuntime>);

impl CoreAgent<'_> {
    /// Begin a turn. Nothing runs until [`Turn::send`].
    pub fn turn(&self, message: impl Into<String>) -> Turn {
        Turn::new(TurnTarget::Runtime(Arc::clone(self.0)), message)
    }

    /// Run a turn with no options — the shortest path from a prompt to a reply.
    pub async fn run(&self, message: impl Into<String>) -> Result<TurnOutcome, CoreError> {
        self.turn(message).send().await
    }
}
