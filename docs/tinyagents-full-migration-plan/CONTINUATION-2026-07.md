# TinyAgents Migration — Continuation Plan (2026-07-03)

Status: supersedes the ordering in `README.md` for remaining work. Written
after a ground-truth audit of `main` (post-#4249), a re-inventory of the
TinyAgents crate (1.4.0 published, 1.5.0 tagged), and a critical
re-evaluation of the `99-deletion-ledger.md` "Never delete" list.

## 1. Where we actually are (ground truth, main @ 2026-07-03)

The "finish TinyAgents harness migration" PR (#4249 → #4399) has landed.
Per-workstream state:

| Workstream       | State                                                                                                                                                                                                                                                                                                                                                                  |
| ---------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 00 baseline      | Done (1.3.0 + sqlite, rusqlite 0.40) — **needs re-bump to 1.4/1.5**                                                                                                                                                                                                                                                                                                    |
| 01 tooling       | Mostly done. Live: crate-internal tool side-lookup (01.1), `tool_filter.rs` 299 + `tool_prep.rs` 344 (01.3)                                                                                                                                                                                                                                                            |
| 02 models        | Mostly done. Live: `reliable.rs` 900 (gated on re-architecture), `ThinkingForwarder` residual seams                                                                                                                                                                                                                                                                    |
| 03 context/cache | Done. `context/` is 1.3k of product prompt/stats state                                                                                                                                                                                                                                                                                                                 |
| 04 sessions      | **Primary unfinished work.** Live path is 100% legacy: `transcript.rs` 1347, `migration.rs` 373, `session_io.rs` 463, `session_db/` 4.5k, `subagent_sessions/` 653, `session_import/` 1.9k. Crate `Store`/`AppendStore` used only by the write-only importer. 04.3 checkpointer swap IS done in code (`tinyagents/checkpoint.rs` deleted) — the 04.3 doc text is stale |
| 05 events        | 05.2 engine deletion done. `progress_tracing` (817 + 477 langfuse + 719 tests) still the live web-progress exporter; journal projection not at parity                                                                                                                                                                                                                  |
| 06 cost          | Not wired: no `BudgetMiddleware`/`BudgetLimits`/`RunTree` in the shared runner. Blocker: `UsageRecorded` de-duplication                                                                                                                                                                                                                                                |
| 07 subagents     | Diagnostic-skeleton graph only; procedural runner still live. `running_subagents.rs` has **grown to 1931 lines** (target ≤300); `JsonlTaskStore` adopted for detached lifecycle records; `run_queue/` + `spawn_depth_context.rs` still live                                                                                                                            |
| 08 orchestration | 08.1/08.2/08.4 done. Live: 08.3 durable interrupts for approvals, 08.5 `worktree_context.rs` fallback thread                                                                                                                                                                                                                                                           |
| 09 embeddings    | Done                                                                                                                                                                                                                                                                                                                                                                   |
| 10 registry      | Pending — `CapabilityRegistry` unused in src/                                                                                                                                                                                                                                                                                                                          |
| 11 testing       | Conformance pass green on 2026-07-02 baseline                                                                                                                                                                                                                                                                                                                          |

Crate APIs available but **unused** in src/: `JsonlTaskStore` (now partially
adopted), `BudgetMiddleware`, `ContextualToolSelectionMiddleware` (only its
shadow), `UnknownToolPolicy`, `CapabilityRegistry`.

## 2. Crate delta: 1.4.0 / 1.5.0 (the upgrade this plan targets)

- **1.4.0 (published 2026-07-02)**
  - `graph::goals` — durable per-thread `ThreadGoal`: completion contract,
    token budget, Active/Paused/BudgetLimited/Complete, `goal_gate_node`
    self-driving loop, `run_continuation_tick`, `note_user_turn`, model tools
    `goal_get/goal_set/goal_complete`, host `goal_pause/resume/clear`.
    Persists on harness `Store` ns `graph.goals`.
  - `graph::todos` — `TaskBoard` kanban (Todo→Ready→InProgress→Done, Blocked,
    AwaitingApproval), single-`InProgress` invariant, `claim_card` CAS,
    single multiplexer `TodoTool`. Persists ns `graph.todos`.
  - Graph resilience: `CompiledGraph::with_node_retry(RetryPolicy)`,
    opt-in backoff sleeping, failure-boundary checkpoints +
    `CompiledGraph::retry(thread)` resumable failures,
    `GraphEvent::NodeRetryScheduled`.
- **1.5.0 (tagged 2026-07-03, not yet on crates.io)**
  - `harness::no_progress::NoProgressTracker` — extracted from OpenHuman
    PR #4389. Pure state machine: `record(step, &ToolAttempt) ->
Continue/Nudge/Halt`; identical-failure ladder, varied-failure backstop,
    fast-trip on hard policy rejects.
- Known crate limitation relevant here: `Store` has no compare-and-set, so
  goals/todos mutations must funnel through one process (fine — the core is
  the single writer).

## 3. Re-evaluated component verdicts (revises `99-deletion-ledger.md`)

The old "Never delete" list over-protects. Revised verdicts, ordered by
reclaimable lines (non-test; tests roughly double each figure):

### Migrate to crate, then delete locally (upstream-extraction candidates)

Precedent: `NoProgressTracker` was extracted upstream from #4389 and shipped
in 1.5.0. Same play for:

| Component                                                     | Lines (~generic) | Notes                                                                                                                                                                                             |
| ------------------------------------------------------------- | ---------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `pformat.rs` + `dispatcher.rs` + `harness/parse.rs`           | 1941 (~1900)     | Tool-call dialect machinery (P-format encoder, permissive XML/JSON parser with key-drift recovery). Zero DomainEvent coupling. Delete gate: 04.2 read-cutover (no live path parses provider text) |
| `multimodal.rs`                                               | 1690 (~1550)     | `[IMAGE:]`/`[FILE:]` marker → provider content blocks, mime allowlist, PDF extraction, fetch gating, truncation budget. Only the marker convention is product                                     |
| `progress_tracing.rs` + `langfuse.rs`                         | 1294 (~1200)     | Crate already ships Langfuse exporters + journals. Delete gate: 05.3 journal-backed web-progress parity                                                                                           |
| `tool_result_artifacts/`                                      | 588 (~500)       | Already built on crate `Store`; overflow-to-artifact is a generic harness concern. Product residue: PII scrub hook                                                                                |
| `hooks.rs` + `stop_hooks.rs` trait machinery                  | 543 (~450)       | PostTurnHook/StopHook traits are pure harness; product hook bodies stay                                                                                                                           |
| `host_runtime.rs` adapter core                                | 456 (~350)       | Native/Docker `RuntimeAdapter` overlaps crate workspace isolation                                                                                                                                 |
| `ArgRecoveryMiddleware`, `RepeatedToolFailureMiddleware` core | ~300             | Latter becomes a thin driver over crate `NoProgressTracker` (1.5.0)                                                                                                                               |
| `tool_filter.rs` fuzzy ranker                                 | 299 (~250)       | Generic ranking; Composio input types stay product                                                                                                                                                |
| `triage/` evaluator+routing+decision core                     | ~800 of 3779     | Generic "LLM triage node" (tiered fallback, cache, verdict parse). `envelope.rs`/`escalation.rs`/`events.rs` stay product (Composio, DomainEvents, agent ids)                                     |

### Replace with crate features, then delete (no upstreaming needed)

| Component                                                                                                                                                                                                     | Lines             | Replaced by                                                                                                                                                       |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `thread_goals/`                                                                                                                                                                                               | 2185              | `graph::goals` (1.4.0). OpenHuman keeps RPC schemas + UI projection over the crate store                                                                          |
| `agent/task_board.rs` + `tools/todo.rs` mechanics                                                                                                                                                             | 604 + ~430        | `graph::todos` `TaskBoard` + `TodoTool` (1.4.0). RPC shapes (`threads.task_board_*`, `openhuman.todos_*`) become projections                                      |
| `task_dispatcher/` executor mechanics                                                                                                                                                                         | ~450 of 1494      | `claim_card` CAS + graph runner node; DomainEvent emission + channel bindings stay                                                                                |
| 6 duplicate middlewares in `tinyagents/middleware.rs` (2448 total): `OpenHumanToolExposureShadow`, `CacheAlign`, `Microcompact`, `CostBudget`, `PromptCacheSegment` (partial), `ToolPolicyMiddleware` (local) | ~900              | Crate `ContextualToolSelectionMiddleware`, cache guard, compaction, `BudgetMiddleware`, `ToolAllowlistMiddleware`. All are self-acknowledged shadows/parity holds |
| `spawn_depth_context.rs`                                                                                                                                                                                      | 92                | Crate `RecursionPolicy`/`RecursionStack`                                                                                                                          |
| legacy session stack (04.2 phase 4)                                                                                                                                                                           | ~9000 incl. tests | Crate `Store`/`AppendStore`/`StoreChatHistory`                                                                                                                    |

### Confirmed product — keep (ledger was right)

`archivist/` (memory/learning policy — only the hook-scheduling shell ~200
lines is generic), `bus.rs`, `schemas.rs`, `error.rs`, `turn_origin.rs`,
`memory_context.rs`, `debug/`, `library/`, `progress.rs` event vocabulary,
prompts **content** (the section/render machinery ~600 lines is generic but
decoupling is low-value), preference/`run_workflow`/`delegate_to_personality`
tools, approval/`MemoryProtocol`/`CliRpcOnly`/`ToolOutcomeCapture`
middlewares.

Cross-cutting gate for the coupled middle tier (session turn/builder, triage
escalation, task dispatch): **DomainEvent subscribers and JSON-RPC shapes** —
these must become projections before their hosts can shrink.

### Coverage sweep: harness files previously unreferenced by any plan doc (2026-07-03)

A basename grep of `agent/harness/**` against this plan folder found these
non-test files with no verdict anywhere. Assigned now (tests follow parents):

| File | Lines | Verdict |
| --- | --- | --- |
| `subagent_runner/extract_tool.rs` | 612 | **[migrate/delete]** — generic progressive-disclosure Q&A over handoff-cached payloads; pairs with `handoff.rs`. Goes with the C5 overflow-to-artifact/handoff extraction |
| `subagent_runner/handoff.rs` | 287 | **[migrate/delete]** — generic oversized-result handoff cache (crate tool-output/artifact overlap). C5 |
| `subagent_runner/types.rs` | 232 | **[shrink]** — spawn options/outcome/error taxonomy largely maps to crate `SubAgentPolicy`/`OrchestrationTaskKind`. C6 |
| `subagent_runner/autonomous.rs` | 29 | **[keep]** — approval-gating override policy for unattended skill runs |
| `fork_context.rs` | 226 | **[delete-via-crate]** — task-local parent-context carrier working around the `Tool` trait; replace with `ToolExecutionContext`/`RunContext` fields (same play that deleted `worktree_context.rs`). C6 |
| `sandbox_context.rs` | 94 | **[delete-via-crate]** — same task-local pattern (sandbox_mode carrier). C6 |
| `task_recency_context.rs` | 106 | **[delete-via-crate]** — same pattern (Composio recency window). C6 |
| `turn_attachments_context.rs` | 71 | **[delete-via-crate]** — same pattern (vision-attachment forwarding). C6 |
| `session/turn_checkpoint.rs` | 105 | **[delete]** with the C1 session cutover (turn checkpointing rides crate checkpoints) |
| `memory_protocol.rs` | 386 | **[keep]** — product memory-protocol state machine (#4116) behind `MemoryProtocolMiddleware` |
| `builtin_definitions.rs` | 273 | **[keep]** — product agent-definition data facade |
| `definition_loader.rs` | 295 | **[keep]** — product TOML definition loader |
| `archivist/{hook_impl,recap,tree_ingest}.rs` | 592 | **[keep]** — archivist internals, verdict inherited from §3 |

The four task-local context carriers (`fork_context`, `sandbox_context`,
`task_recency_context`, `turn_attachments_context`, ~500 lines) are one
C6 work item: extend the crate execution context instead of task-locals.

## 4. Continuation workstreams (execution order)

Sized for `/goal` execution like the original folders; each step lands code +
tests + deletions + a ledger tick.

### C0 — Crate bump to 1.4 (then 1.5 when published)

1. Bump both Cargo worlds to `tinyagents = "1.4"`; re-verify sqlite chain.
2. When 1.5.0 publishes: bump again; rewrite `RepeatedToolFailureMiddleware`
   as a driver over `harness::no_progress::NoProgressTracker`; delete the
   in-house identical-failure ladder.
3. Adopt `with_node_retry` + `CompiledGraph::retry` on the delegation and
   spawn-parallel graphs (replaces bespoke retry glue; complements 08.3).
4. Update `00-baseline.md` "1.3.0 delta" → 1.4/1.5 delta; refresh
   `docs/tinyagents-sdk-gaps.md` (goals/todos and no-progress close two
   OpenHuman-convergence items).

### C1 — Sessions cutover (04.2 phases 2–4) — biggest single unlock

The importer (`session_import/`) already proves shape parity. Execute:

1. 04.1 live dual-writes (turns append `session.{stem}.messages` + descriptor
   upsert alongside legacy JSONL).
2. Shadow reads + parity fixtures (11-fixture matrix), then flip reads.
3. Retire: `transcript.rs` (1347+978), `migration.rs` (373+170),
   `session_io.rs` (463), `session_db/` generic parts (~1.6k),
   `subagent_sessions/` (653); `session_import/` one release later.
4. Then (unblocked by "no live path parses provider text"): delete
   `dispatcher.rs` + `parse.rs` + `pformat.rs` (~1.9k + tests), after
   upstreaming the dialect machinery (C5) or accepting native-only.
   ~11k lines total.
5. Fix the stale 04.3 doc (checkpointer swap already landed; `checkpoint.rs`
   is gone).

### C2 — Thread goals + thread tasks onto `graph::goals`/`graph::todos`

1. Adapter: `thread_goals/store.rs`+`runtime.rs`+`continuation.rs` →
   crate `ThreadGoal` + `goal_gate_node` + `run_continuation_tick`; keep
   `thread_goals/schemas.rs` RPC shapes as projections; map
   `goal_get/set/complete` model tools + host pause/resume/clear.
2. One-time migration of existing goal rows into ns `graph.goals`.
3. `task_board.rs` → crate `TaskBoard`; `tools/todo.rs` → crate `TodoTool`;
   `task_dispatcher/` claim/poll loop → `claim_card` CAS + runner node.
   DomainEvent emissions + `threads.task_board_*`/`openhuman.todos_*` RPC
   become read-side projections of the crate store.
4. Single-writer constraint honoured (core is the only mutator; document it).
5. Delete: `thread_goals/{store,runtime,continuation}.rs` mechanics,
   `task_board.rs`, todo CRUD mechanics, dispatcher executor mechanics
   (~3.2k + tests).

### C3 — Middleware de-duplication (01.1/01.3/06 finish)

1. Flip `ContextualToolSelectionMiddleware` from shadow to owner (parity logs
   are already accumulating); delete `OpenHumanToolExposureShadowMiddleware`,
   then `tool_filter.rs` mechanics + `tool_prep.rs` selection half.
2. De-duplicate `UsageRecorded` (single owner: crate event → bridge records
   once), then install crate `BudgetMiddleware`/`BudgetLimits`; delete local
   `CostBudgetMiddleware` + `turn_subagent_usage.rs` task-local (206) in
   favour of `RunTree` rollup.
3. ~~Delete `CacheAlign` + `Microcompact`~~ CORRECTED by C3 execution
   (2026-07-03): `CacheAlign` deleted (warn-only, crate cache guard already
   installed — commit on `feat/tinyagents-c3-middleware-dedupe`).
   `Microcompact` is NOT crate-superseded — tinyagents 1.5.0 has no
   tool-result body-clearing equivalent and the local one is live on the
   session turn path — moved to the C5 upstream-extraction batch (extract a
   crate microcompact first, then delete locally).
4. ~~Adopt `UnknownToolPolicy::Rewrite`~~ CORRECTED by C3 execution: the
   sentinel + `UnknownToolRewriteMiddleware` were already deleted in 01.2;
   live policy is `UnknownToolPolicy::ReturnToolError`, which preserves the
   attempted tool name + args (#4419 UX). Rewrite mode would regress (needs a
   catch-all target tool); rationale comment added at `run_policy_for`. Done —
   no further action.
   Net: `tinyagents/middleware.rs` 2448 → ~1500 (Microcompact stays until C5).

### C4 — Events/progress projection (05.3) then delete progress_tracing

1. Journal-backed web-progress projection (`HarnessEventJournal` +
   `HarnessStatusStore` + late-attach `replay_from`).
2. Parity vs `SpanCollector` spans; then delete `progress_tracing.rs` +
   `progress_tracing/` (~2k incl. tests) — Langfuse rides crate exporters.
3. DomainEvent/AgentProgress become projections (ledger final clause).

### C5 — Upstream extraction batch (tinyagents PRs, then local deletes)

In crate-repo PRs, mirroring the NoProgressTracker precedent, in value order:

1. `multimodal` attachment resolver (~1550) — marker convention stays here.
2. Tool-call dialect layer (`pformat`/parser) (~1900) — enables C1 step 4 to
   be a pure delete.
3. Overflow-to-artifact tool-result store (~500).
4. PostTurnHook/StopHook trait machinery (~450); host runtime adapter (~350);
   fuzzy tool ranker (~250); ArgRecovery (~150); triage evaluator core
   (~800) as a generic "LLM triage node".
   Each: crate PR → bump → local adapter shrinks to product residue → delete.

### C6 — Subagents finish (07)

1. Absorb `ops/runner.rs` (1212) + `ops/graph.rs` (1039) into named pipeline
   nodes (07.1); procedural runner retired.
2. `running_subagents.rs` 1931 → ≤300: status/tombstone persistence fully on
   `JsonlTaskStore` + `orchestrate_*` tools (07.2).
3. Steering: crate `SteeringRegistry` owns lanes; split then delete
   `run_queue/` (317); `spawn_depth_context.rs` → `RecursionPolicy` (07.3).
4. Replace the four task-local context carriers (`fork_context`,
   `sandbox_context`, `task_recency_context`, `turn_attachments_context`,
   ~500 lines) with typed fields on the crate execution context (see §3
   coverage sweep); shrink `subagent_runner/types.rs` onto crate
   `SubAgentPolicy`/task types.

### C7 — Remaining gated items

- 08.3 durable approval interrupts (now easier with 1.4.0 resumable
  failures); 08.5 drop the `worktree_action_dir` fallback thread, delete
  `worktree_context.rs` remnant wiring.
- 10 CapabilityRegistry projection + fail-closed diagnostics.
- 02.2 `reliable.rs`: unchanged verdict — gated on routing non-turn provider
  calls through the crate harness; do not force.
- `ThinkingForwarder`: delete when crate `ModelDelta` grows reasoning +
  tool-name-on-start (file upstream issue; sdk-gaps §3).

## 5. Expected reclaim

| Phase                 | Local lines deleted (incl. tests, rough)                                     |
| --------------------- | ---------------------------------------------------------------------------- |
| C1 sessions + dialect | ~11,000                                                                      |
| C2 goals/todos        | ~4,000                                                                       |
| C3 middleware dedupe  | ~1,500                                                                       |
| C4 progress tracing   | ~2,000                                                                       |
| C5 upstream batch     | ~6,000                                                                       |
| C6 subagents          | ~4,500                                                                       |
| Total                 | **~29,000** (of ~57k in `agent/` + ~11k adapter + ~9k session/goals modules) |

## 6. Rules (unchanged)

Approval/security/sandbox/credential boundaries inviolate; JSON-RPC contracts
stable unless a migration note lands; adapter → proven parity → delete;
explicit `git add <paths>`; verify branch before commit. Tests deferred per
workstream slice with a final conformance pass (11).
