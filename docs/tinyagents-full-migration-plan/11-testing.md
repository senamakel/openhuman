# 11 — Testing & conformance (run last, per user preference)

Tests are deferred to the end of each workstream slice, with this final
consolidation pass.

## Parity matrix (crate route)

Chat turn, channel turn, sub-agent turn, unknown-tool recovery (RunPolicy),
approval denial, streaming text + reasoning + tool-arg deltas, early-exit
pause, model-call cap checkpoint, budget stop, fallback selection,
compression at 90% window, cache hit/miss, steering inject/cancel,
detached spawn→restart→wait, worktree-isolated parallel run.
Extend `src/openhuman/tinyagents/tests.rs` + `tests/agent_harness_e2e.rs`.

## Testkit adoption

Port legacy loop-wording assertions to crate testkit: `MockModel`
(`with_responses`, `with_tool_call`, `call_count`), graph `assert_graph`/
`GraphEventRecorder`/node doubles (`scripted_route_node`,
`interrupting_node`, `subagent_fake_node`), `RecordingListener` for event
assertions.

## Conformance

- Checkpointer: File vs Sqlite same interrupt/resume behavior (04.3).
- TaskStore: InMemory vs Jsonl lifecycle/filter/restart parity (07.2).
- Graph: `Send` fanout order, failure policy, recursion caps, resume
  (08.x); fuzz-style small-graph composition if time allows.

## Known debts to clear here

- 2 failing detached-orchestrator e2e (see 07.2 step 6).
- `cost::init_global` OnceCell flakiness — test helper gating, not global
  records (memory 2026-07-01).
- Coverage gate: ≥80% on changed lines (repo CI rule) applies to every
  workstream PR, not just this pass.
