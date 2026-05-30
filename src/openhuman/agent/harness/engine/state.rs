//! Turn-state observer seam.
//!
//! The engine drives the loop over a `Vec<ChatMessage>` working buffer, but the
//! three callers want to *do* different things around each step:
//!
//! * the channel loop wants nothing extra ([`NullObserver`]);
//! * the subagent wants per-iteration transcript persistence, usage
//!   accumulation into its `AggregatedUsage`, and worker-thread mirroring;
//! * `Agent::turn` wants its `ContextManager` reduction before each dispatch,
//!   transcript persistence, and `ConversationMessage` history reconciliation.
//!
//! [`TurnObserver`] is the seam: every method defaults to a no-op, so an impl
//! only overrides the hooks its caller needs. The engine still owns the
//! universal concerns (stop hooks, context guard, token-budget trim, the
//! circuit breaker) inline — the observer is for caller-specific side effects.

use anyhow::Result;
use async_trait::async_trait;

use crate::openhuman::inference::provider::{ChatMessage, UsageInfo};

#[async_trait]
pub(crate) trait TurnObserver: Send {
    /// Called before each provider dispatch, after the engine's own context
    /// guard + token-budget trim. `Agent::turn` runs its `ContextManager`
    /// reduction chain here. Default: no-op.
    async fn before_dispatch(&mut self, _history: &mut Vec<ChatMessage>, _iteration: usize) -> Result<()> {
        Ok(())
    }

    /// Called once per provider response that carried a usage block, so the
    /// caller can accumulate its own token tally / transcript usage snapshot.
    fn record_usage(&mut self, _model: &str, _usage: &UsageInfo) {}

    /// Called after each iteration's assistant + tool-result messages have been
    /// appended to `history` (and after the final assistant message). The
    /// subagent and `Agent` persist their transcript here.
    fn after_history_commit(&mut self, _history: &[ChatMessage], _iteration: usize) {}

    /// Mirror a (user, assistant) round to a worker thread. Subagent-only;
    /// default no-op.
    fn on_worker_round(&mut self, _assistant_text: &str, _tool_results: &str) {}
}

/// No-op observer for the channel/CLI/triage loop, which keeps no extra state.
pub(crate) struct NullObserver;

impl TurnObserver for NullObserver {}
