# 02.3 — Native reasoning + tool-arg streaming

tinyagents 1.2.0 `MessageDelta` has a `reasoning` field and
`StreamAccumulator::reasoning()`; `ModelStreamItem::ToolCallDelta` streams
tool-arg fragments. The out-of-band `ThinkingForwarder` is deletable.

## Steps

1. In `src/openhuman/tinyagents/model.rs`, map provider thinking deltas to
   `MessageDelta::reasoning(...)` on the crate stream (today they bypass it).
2. In `OpenhumanEventBridge` (`observability.rs`), source thinking-progress
   projection from the crate stream items instead of the forwarder.
   KNOWN CRATE GAP: the middleware-facing `harness::model::ModelDelta` has no
   reasoning field, and the agent loop currently drops reasoning when converting
   stream items into that middleware shape. `AgentEvent::ModelDelta` does carry
   `MessageDelta.reasoning`, so the bridge can consume the event stream once the
   provider adapter maps thinking deltas onto crate stream items.
3. Map provider tool-call argument fragments onto `ToolCallDelta` for UI
   tool-timeline assembly.
4. Verify sub-agent child thinking deltas still reach the scope-aware bridge
   (parity with the current forwarder behavior).

## Deletions

- `ThinkingForwarder` (in `tinyagents/model.rs`/`mod.rs`) and its plumbing
  through `run_turn_via_tinyagents_shared`.

## Acceptance

- UI receives visible-text, reasoning, and tool-arg deltas from crate stream
  items alone; streaming e2e (chat + sub-agent) green.
- Non-streaming providers emit post-hoc reasoning once.
