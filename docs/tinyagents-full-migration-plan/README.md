# TinyAgents Full Migration Plan

Status: active plan (2026-07-02). Branch: `issue/4249-finish-tinyagents-migration`.

> **2026-07-03 update:** #4249 has landed on main. Remaining work is
> re-planned in [`CONTINUATION-2026-07.md`](CONTINUATION-2026-07.md), which
> supersedes the workstream ordering below and targets tinyagents 1.4/1.5
> (`graph::goals`, `graph::todos`, `NoProgressTracker`, resumable graph
> failures).
>
> **2026-07-04 update:** vendor-crate improvement work (reasoning tokens,
> streaming, performance, native Anthropic provider, upstream extractions)
> is planned separately in
> [`../tinyagents-vendor-improvement-plan/`](../tinyagents-vendor-improvement-plan/)
> (workstreams V1–V6, run in parallel with C0–C7; this folder's
> `99-deletion-ledger.md` remains the master delete list).

Goal: **hard-migrate** OpenHuman's agent harness onto the `tinyagents` crate as
the library for orchestration, caching, tooling, observability, model
providers, context management, embeddings, sub-agents, steering, summarization,
events, and session storage — then **delete the legacy OpenHuman files**.
OpenHuman keeps only product policy (prompts, registry, security/approval
semantics, credentials, UX projections).

Each workstream folder below is sized for `/goal` execution: the folder
`README.md` states scope + done criteria; numbered step files are individual
goals. Execute a step file end-to-end (code + tests + deletions + commit).

## Key facts superseding older docs

- Current crate is **1.5.0**; the plan still records the earlier "1.3.0 delta"
  in `00-baseline.md` where those primitives first became available.
- `docs/tinyagents-sdk-gaps.md` was refreshed against TinyAgents 1.3.0 and
  should be re-audited after the 1.5.0 bump; it currently
  tracks only residual gaps. tinyagents 1.2.0-1.3.0 ships `UnknownToolPolicy`,
  `ToolPolicy` safety metadata + `ToolPolicyMiddleware`, reasoning deltas
  (`MessageDelta.reasoning`), durable `JsonlTaskStore` + orchestration tools,
  `graph::parallel::map_reduce`, `ModelCatalog` w/ pricing,
  `WorkspaceIsolation`, `BudgetMiddleware`, event journals/status stores,
  `CapabilityRegistry`, embeddings traits. See `00-baseline.md`.
- Inventory of live/legacy files: `../tinyagents-harness-migration-audit.md`
  plus the per-folder deletion lists here (`99-deletion-ledger.md` is the
  master list).

## Workstreams (suggested order)

| # | Folder | Theme |
|---|--------|-------|
| 0 | `00-baseline.md` | crate bump, rusqlite 0.40, feature flags |
| 1 | `01-tooling/` | ToolPolicy round-trip, unknown-tool, dynamic exposure, output budgets |
| 2 | `02-models/` | ModelRegistry, profiles/catalog, fallback/retry, reasoning stream |
| 3 | `03-context-cache/` | delete legacy context reducers, ResponseCache + prompt-cache guard |
| 4 | `04-sessions/` | transcript → Store/AppendStore cutover, import phases 2–4 |
| 5 | `05-events/` | journals/status canonical, bridge consolidation, DomainEvent projection |
| 6 | `06-cost/` | UsageAccounting/Budget middleware, lineage rollup |
| 7 | `07-subagents/` | SubAgent pipeline, detached TaskStore, steering/recursion |
| 8 | `08-orchestration/` | spawn_parallel graph tool, map_reduce, interrupts, graph-stub cleanup |
| 9 | `09-embeddings.md` | EmbeddingModel/VectorStore/Retriever adapters |
| 10 | `10-registry.md` | CapabilityRegistry projection + diagnostics |
| 11 | `11-testing.md` | parity matrix, testkit, conformance (last) |
| 99 | `99-deletion-ledger.md` | master delete list with preconditions |

## Rules (unchanged from spec)

- Never bypass approval/security/sandbox/workspace/credential boundaries.
- JSON-RPC contracts stay stable unless a migration note lands with the change.
- Adapter first, flip ownership on proven parity, **then delete** — deletion is
  mandatory, not optional; every step file names its deletions.
- Stage files explicitly (`git add <paths>`), never `git add -A` (see memory).
- Verify `git branch --show-current` before each commit.
