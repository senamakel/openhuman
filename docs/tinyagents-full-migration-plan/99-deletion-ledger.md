# 99 — Deletion ledger (master list)

Hard-migration means these files GO. Each row names its precondition step.
Rule: call-site search + parity coverage before every delete; tick rows as
they land.

## Immediately deletable (dead/vestigial — no precondition beyond call-site search)

- [x] `agent/memory_loader.rs` (5-line facade) — 09
- [x] `agent/tree_loader.rs` (210, unwired per #3170) — 09
- [x] `harness/compaction/mod.rs` shim — 03.1

## Superseded by existing middlewares (verify + delete)

- [x] `harness/compaction/cache_align.rs` (200) — 03.1/03.2
- [x] `tokenjuice::compact_tool_output` default-Full wrapper — 01.4
- [x] `context/microcompact.rs` (269) — 03.1
- [x] `context/pipeline.rs` (454) + `context/guard.rs` (236, keep stats structs) — 03.1
- [x] `context/tool_result_budget.rs` (172) — 03.1
- [x] `harness/payload_summarizer.rs` (490) — 01.4

## Deletable after SDK-surface adoption

- [x] `UNKNOWN_TOOL_SENTINEL` + `UnknownToolRewriteMiddleware` — 01.2
- [ ] crate-internal tool side-lookup in `tinyagents/middleware.rs` — 01.1
      (live overlays for args-aware effects/permissions, CLI/RPC scope, and
      generated runtime context until SDK policy metadata can represent them)
- [ ] `harness/tool_filter.rs` mechanics (299) +
      `subagent_runner/tool_prep.rs` (344) — 01.3
      (live until middleware owns child/toolkit selection; `tool_prep.rs` also
      contains non-filter prompt helpers that must move first)
- [ ] `ThinkingForwarder` — 02.3
      (streaming reasoning moved to `MessageDelta.reasoning`; tool-arg progress
      and non-streaming reasoning fallback remain live inside the tinyagents adapter)
- [ ] `inference/provider/reliable.rs` (1215 + 1443 tests) — 02.2
      (live provider-factory wrapper; TinyAgents retry is held at one attempt
      until fallback routes, retry events, memory-tree wrappers, and shared
      classifier exports are replaced)
- [x] `tinyagents/orchestration.rs::run_parallel_fanout` — 08.1
- [x] `harness/engine/` (entire dir, 309) — 05.2
- [ ] `agent/progress_tracing.rs` + tests (1338, if duplicate) — 05.2
      (live web-progress exporter until journal-backed projection reaches parity)
- [ ] `harness/run_queue/` mechanics (317 total; 174 non-test) — 07.3
      (live adapter for detached steer/collect plus web followup/parallel;
      split before deleting)
- [ ] crate-internal `harness/spawn_depth_context.rs` (66) — 07.3
      (live recursion guard for nested delegation)
- [ ] `harness/worktree_context.rs` (74) — 08.5
      (live crate-internal action-root override for parallel worktrees)
- [x] 32 × `agent_registry/agents/*/graph.rs` stubs (~420) + five default-only
      non-registry graph modules — 08.4
- [ ] `tinyagents/checkpoint.rs` (`SqlRunLedgerCheckpointer`, 250) — 04.3
      (live crate-internal adapter until crate checkpointer/schema migration
      replaces `graph_checkpoints`)
- [ ] crate-internal `CostBudgetMiddleware` (`tinyagents/middleware.rs`) +
      crate-internal `agent/harness/turn_subagent_usage.rs` (176) task-local — 06
      (live until crate budget/run-tree accounting avoids duplicate
      `UsageRecorded` and covers parent-turn rollups)
- [ ] `agent/dispatcher.rs` (609) + `harness/parse.rs` (833) legacy tool-call
      parsing — after XML/P-format transcripts read from the store and no
      live path parses provider text (04.2 + verify)
      (live compatibility shell for prompt dialect selection, history
      serialization, XML/P-format fallback parsing, checkpoint cleanup, native
      text fallback, and TinyAgents text-mode provider responses; trim
      unused/test-only parse helpers before full delete)

## Deletable after session-store cutover (04.2 phase 4)

- [ ] `session/transcript.rs` (1347) + tests (978)
- [ ] `session/migration.rs` (373) + tests
- [ ] `session/turn/session_io.rs` (391)
- [ ] `session_db/{ops,store,schemas,types}.rs` generic parts (~1.6k)
- [ ] `agent_orchestration/subagent_sessions/` (~650)
- [ ] `src/openhuman/session_import/` (~1.7k) + RPC controller — one
      release after auto-import ships

## Shrink (not full delete)

- `session/agent_tool_exec.rs` 471 → delete test-only parity shim after
  direct-executor tests migrate — 01.4
- `session/turn/tools.rs` 697 → parent-context/assembly glue — 01.3
  (dynamic delegation refresh and skill-event catalogue reconciliation remain
  live until middleware owns contextual selection)
- `subagent_runner/ops/*` 2764 (+1827 companion tests) → graph nodes/tests — 07.1
- `running_subagents.rs` 1250 → ≤~300 policy/executor glue — 07.2
- `tools/spawn_parallel_agents.rs` is a 128-line tool shell; remaining shrink
  target is `agent_orchestration/spawn_parallel_graph.rs` graph mechanics
  (1280) — 08.2
- `context/` → stats + product prompt state — 03
- `cost/catalog.rs` (622) → catalog snapshot loader once config seeding, cost
  estimates, and `context_window_for_model` all read one catalog projection —
  02.4

## Never delete (product policy)

Prompts/`agent/prompts/`, agent registry definitions, security/approval
semantics, credentials/factory/router names, triage, task board/dispatcher,
archivist, memory stores, worktree policy, `host_runtime.rs`, multimodal
policy, JSON-RPC shapes, `DomainEvent` until all subscribers move.
