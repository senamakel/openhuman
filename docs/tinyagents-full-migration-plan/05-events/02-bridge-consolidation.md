# 05.2 — One event bridge; delete engine/

Three parallel progress paths exist: `OpenhumanEventBridge`
(`tinyagents/observability.rs`), `harness/engine/progress.rs`
(`ProgressReporter`/`TurnProgress`/`SubagentProgress`), and scattered direct
`publish_global` calls in session/turn code.

## Steps

1. Inventory `ProgressReporter`/`TurnProgress` call sites; re-express each
   projection through `OpenhumanEventBridge` (it already maps `AgentEvent` →
   `AgentProgress` + cost tracker). Child/sub-agent progress uses the
   scope-aware bridge.
2. Done: `engine/checkpoint.rs` is gone. `CapPauser` owns the graceful
   max-iteration stop, and the remaining sub-agent checkpoint summary is
   localized in `subagent_runner/ops/checkpoint.rs`.
3. Sweep direct `publish_global(DomainEvent::Agent*)` calls on turn paths
   (session/turn, agent_tool_exec, orchestration tools) — emit through the
   bridge or a typed helper so ordering/rate-limiting is single-owner.
4. Current finding (2026-07-02): `agent/progress_tracing.rs` is not currently
   redundant. It is the opt-in `observability.agent_tracing` exporter fed by
   the web progress bridge's `AgentProgress` stream. Retain it until the 05.1
   journal path can produce the same metadata-only OTel/Langfuse spans and
   append/export contract.

## Deletions

- `src/openhuman/agent/harness/engine/` (entire dir: mod, checkpoint,
  progress — 309 lines).
- Deleted: `engine/checkpoint.rs`; retain `engine/progress.rs` while
  `ProgressReporter`/`TurnProgress` call sites remain.
- Redundant publishes found in step 3.
- Retain `agent/progress_tracing.rs` for now; delete only after journal-backed
  span export proves parity with the current config contract.

## Acceptance

- Progress-event parity fixture: same `AgentProgress` sequence for a
  scripted turn before/after (tool timeline, thinking, cost footer).
- No `engine::` imports remain.
