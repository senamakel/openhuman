# 07.2 — Detached sub-agents on durable TaskStore

`running_subagents.rs` already mirrors lifecycle into the crate TaskStore but
still owns watch channels, abort handles, task lookup, and ownership checks. The
crate now has `JsonlTaskStore` (durable), `OrchestrationTaskSpec::with_lineage`,
filters, and a `SteeringRegistry`.

Current status (2026-07-02): detached sub-agent lifecycle records now open a
per-workspace durable `JsonlTaskStore` at
`<workspace_dir>/.openhuman/orchestration_tasks.jsonl` on first spawn, falling
back to `InMemoryTaskStore` only if that workspace log cannot be created/opened.
Records carry parent session, parent thread, durable `subagent_session_id`, and
workspace metadata, and terminal/cancelled mirrors now resolve the same
workspace-scoped store that recorded the spawn. The executor/control path still
uses OpenHuman's watch channels, abort handles, task lookup, and `RunQueue`;
the unused future `running_subagents::close` hook has been removed and the
test-only typed ledger snapshot plus finished background completion/delivery
queues are crate-internal. `running_subagents` and `subagent_sessions` still
remain public because the live `harness-subagent-audit` binary imports their
debug/control types directly; shrink that audit seam before claiming the
registry/session APIs are crate-only. Restart reconciliation and
steering-registry replacement remain pending.

## Steps

1. Done: detached lifecycle now opens `JsonlTaskStore::open` under the
   workspace store dir, with `InMemoryTaskStore` only as the open-failure
   fallback. Remaining: task records carry lineage
   (`with_lineage(parent_run_id, root_run_id)`) and timeout fields from the
   parent run context; thread/session metadata is already live.
2. Re-express controls on crate semantics: wait → `orchestrate_await`-style
   store polling/wait handle; cancel/kill → `CancellationToken` +
   terminal record; steer → `SteeringRegistry` (`TaskId → SteeringHandle`)
   replacing the RunQueue lookup plumbing. Keep abort-handle hard-kill as
   the OpenHuman executor detail.
3. Keep OpenHuman ownership checks + durable session rows as policy over
   the store; cancelled/failed states become terminal `OrchestrationTaskRecord`s.
4. Restart/resume: on boot, reconcile `JsonlTaskStore` live records against
   actual executors (orphans → failed-with-restart marker); prove desktop
   restart parity vs today's behavior. Run the crate's 1.3.0 testkit
   contracts (`taskstore_concurrent_contract`, `taskstore_replay_contract`)
   against the chosen store.
5. Evaluate exposing crate `orchestrate_*` tools to the orchestrator agent
   as the internal engine under `spawn_agent`/`wait_agents`/etc. — the
   product tools stay as the model-visible surface (names/output shapes are
   product contract).
6. While here: root-cause the 2 known-failing detached-orchestrator e2e
   hangs (`e2e_orchestrator_answers_coding_agent_question...` — child stays
   Running past timeout; see memory 2026-06-30f).

## Deletions

- Watch-channel/task-lookup mechanics in `running_subagents.rs` (1250)
  (target ≤ ~300 lines of policy + executor glue).

## Acceptance

- Spawn detached → restart process → list/wait/cancel still work.
- running_subagents suite (12) + orchestration tools suite (111) green,
  incl. the 2 currently-failing e2e.
