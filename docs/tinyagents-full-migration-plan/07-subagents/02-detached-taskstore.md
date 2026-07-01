# 07.2 — Detached sub-agents on durable TaskStore

`running_subagents.rs` (1024) already mirrors lifecycle into
`InMemoryTaskStore` but still owns watch channels, abort handles,
tombstones, ownership checks. The crate now has `JsonlTaskStore` (durable),
`OrchestrationTaskSpec::with_lineage`, filters, and a `SteeringRegistry`.

## Steps

1. Swap `InMemoryTaskStore` → `JsonlTaskStore::open` under the workspace
   store dir; task records carry lineage (`with_lineage(run/root/parent)`),
   thread, timeout.
2. Re-express controls on crate semantics: wait → `orchestrate_await`-style
   store polling/wait handle; cancel/kill → `CancellationToken` +
   terminal record; steer → `SteeringRegistry` (`TaskId → SteeringHandle`)
   replacing the RunQueue lookup plumbing. Keep abort-handle hard-kill as
   the OpenHuman executor detail.
3. Keep OpenHuman ownership checks + durable session rows as policy over
   the store; tombstones become terminal `OrchestrationTaskRecord`s.
4. Restart/resume: on boot, reconcile `JsonlTaskStore` live records against
   actual executors (orphans → failed-with-restart marker); prove desktop
   restart parity vs today's behavior.
5. Evaluate exposing crate `orchestrate_*` tools to the orchestrator agent
   as the internal engine under `spawn_agent`/`wait_agents`/etc. — the
   product tools stay as the model-visible surface (names/output shapes are
   product contract).
6. While here: root-cause the 2 known-failing detached-orchestrator e2e
   hangs (`e2e_orchestrator_answers_coding_agent_question...` — child stays
   Running past timeout; see memory 2026-06-30f).

## Deletions

- Watch-channel/tombstone/task-lookup mechanics in `running_subagents.rs`
  (target ≤ ~300 lines of policy + executor glue).

## Acceptance

- Spawn detached → restart process → list/wait/cancel still work.
- running_subagents suite (12) + orchestration tools suite (111) green,
  incl. the 2 currently-failing e2e.
