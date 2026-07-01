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
2. Move `engine/checkpoint.rs` (`CheckpointStrategy::on_max_iter`) into the
   cap-pause path (`CapPauser` in `tinyagents/mod.rs`) — it's the only
   consumer.
3. Sweep direct `publish_global(DomainEvent::Agent*)` calls on turn paths
   (session/turn, agent_tool_exec, orchestration tools) — emit through the
   bridge or a typed helper so ordering/rate-limiting is single-owner.
4. `agent/progress_tracing/` (616): re-point at journal records (05.1) or
   the bridge; delete the parallel trace bridge if redundant.

## Deletions

- `src/openhuman/agent/harness/engine/` (entire dir: mod, checkpoint,
  progress — 309 lines).
- Redundant publishes found in step 3.
- `agent/progress_tracing/` if step 4 proves it a duplicate.

## Acceptance

- Progress-event parity fixture: same `AgentProgress` sequence for a
  scripted turn before/after (tool timeline, thinking, cost footer).
- No `engine::` imports remain.
