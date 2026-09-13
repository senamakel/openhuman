//! [`MemoryProtocolMiddleware`]: nudge the model back onto the
//! read-index → dedupe → write → update-index memory protocol.

use async_trait::async_trait;

use tinyagents_harness::context::RunContext;
use tinyagents_harness::error::Result as TaResult;
use tinyagents_harness::middleware::{AgentRun, Middleware};
use tinyagents_harness::tool::ToolResult as TaToolResult;
use tinyinference::tool::ToolCall as TaToolCall;

/// Agents are told to follow a **read-index → dedupe → write → update-index**
/// cycle around durable memory, but the contract was never enforced, so it was
/// followed inconsistently: writes landed without a dedupe read (duplicating
/// entries) and `update_memory_md` was skipped (so `MEMORY.md` drifted from the
/// store). This middleware observes the ordered sequence of *successful* memory
/// tool calls via [`MemoryProtocolTracker`] and, on each memory write, appends a
/// corrective note to the tool result so the model is nudged back onto the
/// protocol — the same "structured correction surfaced to the model" pattern the
/// unknown-tool recovery (#4118) uses. At run end it warns when a write was never
/// followed by an index update (the index is left stale).
///
/// Only *successful* ops advance the state machine — a failed `memory_store`
/// neither creates an entry nor obliges an index update. Non-memory tools are
/// ignored, so this is a no-op on turns that never touch memory.
pub struct MemoryProtocolMiddleware {
    tracker: std::sync::Mutex<crate::agent::harness::memory_protocol::MemoryProtocolTracker>,
    /// call_id → classified op, captured in `before_tool` (the tool result carries
    /// no arguments, yet `update_memory_md` and `memory_tree` can only be
    /// classified from their `file` / `mode` argument). Correlated back by
    /// `result.call_id` in `after_tool`.
    pending_ops: std::sync::Mutex<
        std::collections::HashMap<String, crate::agent::harness::memory_protocol::MemoryOp>,
    >,
}

impl MemoryProtocolMiddleware {
    pub fn new() -> Self {
        Self {
            tracker: std::sync::Mutex::new(
                crate::agent::harness::memory_protocol::MemoryProtocolTracker::new(),
            ),
            pending_ops: std::sync::Mutex::new(std::collections::HashMap::new()),
        }
    }
}

impl Default for MemoryProtocolMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Middleware<()> for MemoryProtocolMiddleware {
    fn name(&self) -> &str {
        "memory_protocol"
    }

    async fn before_tool(
        &self,
        _ctx: &mut RunContext<()>,
        _state: &(),
        call: &mut TaToolCall,
    ) -> TaResult<()> {
        // Classify with the arguments in hand (the result won't carry them) and
        // stash the op keyed by call id. Only memory-relevant ops are stored, so
        // the map stays empty on turns that never touch memory.
        let op =
            crate::agent::harness::memory_protocol::classify_memory_op(&call.name, &call.arguments);
        if op != crate::agent::harness::memory_protocol::MemoryOp::Other {
            if let Ok(mut ops) = self.pending_ops.lock() {
                ops.insert(call.id.clone(), op);
            }
        }
        Ok(())
    }

    async fn after_tool(
        &self,
        _ctx: &mut RunContext<()>,
        _state: &(),
        result: &mut TaToolResult,
    ) -> TaResult<()> {
        // Consume the op captured for this call (removing it so the map can't
        // grow unbounded). Absent → a non-memory tool: nothing to enforce.
        let op = self
            .pending_ops
            .lock()
            .ok()
            .and_then(|mut ops| ops.remove(&result.call_id));
        let Some(op) = op else {
            return Ok(());
        };
        // Only successful memory ops advance the protocol — a failed write did
        // not mutate memory and must not demand an index update.
        if result.error.is_some() {
            return Ok(());
        }
        let observation = {
            let mut tracker = match self.tracker.lock() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
            tracker.observe(op)
        };
        if let Some(note) = observation.guidance(&result.name) {
            tracing::debug!(
                tool = result.name.as_str(),
                missing_index_read = observation.missing_index_read,
                index_drift = observation.index_drift,
                "[tinyagents::mw] memory-protocol guidance appended to tool result"
            );
            if !result.content.is_empty() {
                result.content.push_str("\n\n");
            }
            result.content.push_str(&note);
        }
        Ok(())
    }

    async fn after_agent(
        &self,
        _ctx: &mut RunContext<()>,
        _state: &(),
        _run: &mut AgentRun,
    ) -> TaResult<()> {
        let pending = self
            .tracker
            .lock()
            .map(|tracker| tracker.pending_index_update())
            .unwrap_or(false);
        if pending {
            tracing::warn!(
                "[tinyagents::mw] memory-protocol: run ended with a memory write that was never \
                 followed by update_memory_md — the MEMORY.md index is left stale"
            );
        }
        Ok(())
    }
}
