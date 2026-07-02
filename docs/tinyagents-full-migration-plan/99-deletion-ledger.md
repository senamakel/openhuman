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
- [ ] `context/pipeline.rs` (454) + `context/guard.rs` (236, keep stats structs) — 03.1
- [x] `context/tool_result_budget.rs` (172) — 03.1
- [x] `harness/payload_summarizer.rs` (490) — 01.4

## Deletable after SDK-surface adoption

- [ ] `UNKNOWN_TOOL_SENTINEL` + `UnknownToolRewriteMiddleware` — 01.2
- [ ] tool side-lookup in `tinyagents/middleware.rs` — 01.1
- [ ] `harness/tool_filter.rs` mechanics + `subagent_runner/tool_prep.rs` (350) — 01.3
- [ ] `ThinkingForwarder` — 02.3
- [ ] `inference/provider/reliable.rs` (1215 + 1443 tests) — 02.2
- [ ] `tinyagents/orchestration.rs::run_parallel_fanout` — 08.1
- [ ] `harness/engine/` (entire dir, 309) — 05.2
- [ ] `agent/progress_tracing/` (616, if duplicate) — 05.2
- [ ] `harness/run_queue/` mechanics (345) — 07.3
- [ ] `harness/spawn_depth_context.rs` (66) — 07.3
- [ ] `harness/worktree_context.rs` (74) — 08.5
- [ ] 32 × `agent_registry/agents/*/graph.rs` stubs (~420) — 08.4
- [ ] `tinyagents/checkpoint.rs` (`SqlRunLedgerCheckpointer`, 251) — 04.3
- [ ] `CostBudgetMiddleware` + `turn_subagent_usage.rs` task-local — 06
- [ ] `agent/dispatcher.rs` (609) + `harness/parse.rs` (830) legacy tool-call
      parsing — after XML/P-format transcripts read from the store and no
      live path parses provider text (04.2 + verify)

## Deletable after session-store cutover (04.2 phase 4)

- [ ] `session/transcript.rs` (1347) + tests (978)
- [ ] `session/migration.rs` (373) + tests
- [ ] `session/turn/session_io.rs` (391)
- [ ] `session_db/{ops,store,schemas,types}.rs` generic parts (~1.6k)
- [ ] `agent_orchestration/subagent_sessions/` (~650)
- [ ] `src/openhuman/session_import/` (~1.7k) + RPC controller — one
      release after auto-import ships

## Shrink (not full delete)

- `session/agent_tool_exec.rs` 505 → session bookkeeping only — 01.3/01.4
- `session/turn/tools.rs` 693 → assembly glue — 01.3
- `subagent_runner/ops/*` → graph nodes — 07.1
- `running_subagents.rs` 1024 → ≤~300 policy/executor glue — 07.2
- `tools/spawn_parallel_agents.rs` 809 → thin wrapper — 08.2
- `context/` → stats + product prompt state — 03
- `cost/catalog.rs` → catalog snapshot loader — 02.4

## Never delete (product policy)

Prompts/`agent/prompts/`, agent registry definitions, security/approval
semantics, credentials/factory/router names, triage, task board/dispatcher,
archivist, memory stores, worktree policy, `host_runtime.rs`, multimodal
policy, JSON-RPC shapes, `DomainEvent` until all subscribers move.
