# 00 — Baseline: crate, features, native links

## Steps

1. **Bump `tinyagents` to `"1.3"`** (root `Cargo.toml` pins `"1.2"`; bump the
   requirement + `cargo update -p tinyagents` in both Cargo worlds — root and
   `app/src-tauri/`). Known 1.1→1.2 break already handled
   (`MessageDelta::text` ctor). Note: the `openai` crate feature was removed
   after 1.2.0 (1.2.1+ features are only `sqlite`/`repl`) — we never enabled
   it, so no impact. See "1.3.0 delta" below for new API this plan uses.
2. **Align rusqlite to 0.40** in both worlds (`Cargo.toml:130` root,
   `app/src-tauri/Cargo.toml:145`, currently `0.37` bundled). tinyagents'
   `sqlite` feature pins `rusqlite 0.40` bundled; the `links = "sqlite3"`
   conflict is the only blocker. Fix openhuman call sites for the 0.37→0.40
   API delta, then enable `tinyagents = { version = "1.2", features =
["sqlite"] }`.
3. **Unlocks:** crate `SqliteCheckpointer` → later deletion of
   `src/openhuman/tinyagents/checkpoint.rs` (`SqlRunLedgerCheckpointer`,
   251 lines) once graphs are re-pointed and `graph_checkpoints` rows are
   migrated or expired (see `04-sessions/`).
4. **Do NOT enable `openai` feature** — OpenHuman providers stay the product
   source of truth for credentials/billing (spec non-goal).
5. Update the "TinyAgents crate: features & compatibility" section in
   `gitbooks/developing/architecture/agent-harness.md` and the Cargo.toml
   comment (lines 46–55) after the flip.
6. Mark `docs/tinyagents-sdk-gaps.md` items 1, 2, 3, 4, 7, 10, 11, 12
   as shipped in 1.2.0–1.3.0 (verified against crate source); keep only the
   residuals, re-verified against 1.3.0: no free-form `ToolSchema` metadata
   map, no reasoning field on `AgentEvent::ModelDelta` payload, no
   `root_run_id` on `RunConfig`, no USD field on `Usage`.

## 1.3.0 delta (verified from the published crate source)

Same `rusqlite ^0.40 bundled` pin and feature set as 1.2.1. New API this
plan's workstreams should use directly:

- `AgentEvent::ToolsFiltered { by, excluded }` — exposure decisions are now
  event-native (01.3).
- `AgentEvent::{BudgetReserved, BudgetReconciled}` + `BudgetLimits.
  max_cached_input_tokens` + reservation tracking — pre-spend
  reserve/reconcile (06).
- `AgentEvent::ControlApplied` + `MiddlewareControl::kind()/precedence()` —
  typed middleware control outcomes with defined precedence (sdk-gaps §13
  closed).
- `ToolAllowlistMiddleware::inheriting(...)` — parent→child narrowing
  composition built in (01.3, 07).
- `ToolPolicyMiddleware::{require_sandbox, require_approval,
  enforce_result_bytes}` builders (01.1).
- `ParallelOptions::{with_item_timeout, with_total_timeout,
  with_cancellation}` (08.1/08.2).
- `graph::testkit` TaskStore conformance contracts
  (`taskstore_concurrent_contract`, `taskstore_replay_contract`) (07.2, 11).
- `WorkspaceDescriptor::enforce(path, events)` — violation check that also
  emits events (08.5).
- `ModelSelection.allow_retired`; OpenAI-compat runtime model listing
  (`ModelListing`/`ModelListWire`) for catalog discovery (02.1/02.4).
- Registry: `ComponentKind::{Middleware, Checkpointer, TaskStore, Listener}`,
  alias diagnostics (`AliasBinding`, cross-kind name-reuse) (10).
- `EventRecord::with_stream_id` — stream ids on event records (05.1).

## Acceptance

- Both Cargo worlds `cargo check` clean with `sqlite` feature on.
- One duplicate-free `cargo tree -i libsqlite3-sys` per world.
- Docs updated; sdk-gaps marked.
