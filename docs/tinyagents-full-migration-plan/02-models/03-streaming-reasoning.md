# 02.3 — Native reasoning + tool-arg streaming

tinyagents 1.3.0 `MessageDelta` has a `reasoning` field and
`StreamAccumulator::reasoning()`; `ModelStreamItem::ToolCallDelta` streams
tool-arg fragments. The out-of-band `ThinkingForwarder` is not one-shot
deletable: streaming reasoning can ride the crate stream, but OpenHuman still
uses the forwarder for non-streaming post-hoc reasoning and tool-argument
progress that preserves the UI's `tool_name`/start-event contract.

Current status (2026-07-02): streaming provider `ThinkingDelta` values are
mapped to `MessageDelta::reasoning(...)`, and `OpenhumanEventBridge` projects
`delta.reasoning` into the same parent/sub-agent thinking progress events as the
old forwarder path. Tool-call start/argument fragments still ride
`ThinkingForwarder` until `ToolCallDelta` can preserve OpenHuman's live
tool-name timeline semantics.

## Steps

1. Done for streaming providers: `src/openhuman/tinyagents/model.rs` maps
   provider thinking deltas to `MessageDelta::reasoning(...)` on the crate
   stream, and `OpenhumanEventBridge` (`observability.rs`) projects
   `delta.reasoning` into OpenHuman progress events.
2. Map provider tool-call argument fragments onto `ToolCallDelta` for UI
   tool-timeline assembly.
3. Verify sub-agent child thinking deltas still reach the scope-aware bridge
   (parity with the current forwarder behavior).

## Deletions

- Reasoning side of `ThinkingForwarder`: moved for streaming providers; keep the
  non-streaming fallback until the non-streaming path has an equivalent event.
- Tool-argument side of `ThinkingForwarder` (in `tinyagents/model.rs`/`mod.rs`)
  only after native `ToolCallDelta` preserves `tool_name` and start-event
  semantics for the UI timeline.

## Acceptance

- UI receives visible-text, reasoning, and tool-arg deltas from crate stream
  items alone; streaming e2e (chat + sub-agent) green.
- Non-streaming providers emit post-hoc reasoning once.
