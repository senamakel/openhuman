# 11 — Testing & conformance (run last, per user preference)

Tests are deferred to the end of each workstream slice, with this final
consolidation pass.

## Parity matrix (crate route)

Chat turn, channel turn, sub-agent turn, unknown-tool recovery (RunPolicy),
approval denial, streaming text + reasoning + tool-arg deltas, early-exit
pause, model-call cap checkpoint, budget stop, fallback selection,
compression at 90% window, cache hit/miss, steering inject/cancel,
detached spawn→restart→wait, worktree-isolated parallel run.
Extend `src/openhuman/tinyagents/tests.rs`, `tests/agent_harness_e2e.rs`, and
`tests/agent_tool_loop_raw_coverage_e2e.rs`.

Current execution mode for this migration branch: tests are intentionally
deferred while code/docs are still moving quickly. Use docs drift and cargo
checks for small slices; run the behavioral suites in the final conformance
pass before declaring a workstream done.

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

- Previously flaky detached-orchestrator e2e timeout/hang debt (see 07.2 step
  6): the tight 2s waits are now 15s, but re-run when tests are re-enabled
  before treating that path as proven.
- `cost::init_global` is now idempotent/no-panic; keep future tests isolated
  around process-global state so they do not depend on execution order.
- Coverage gate: ≥80% on changed lines applies when frontend/rust coverage
  lanes are triggered. Docs-only migration-plan markdown does not trigger the
  coverage lane, but code slices still need the normal changed-line coverage
  before PR completion.
