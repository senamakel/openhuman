# 06 — Upstream Extraction: OpenHuman logic to move INTO tinyagents

Extends the old plan's C5 batch with the vendor-submodule workflow: extract
generic harness logic into `vendor/tinyagents`, bump the gitlink, shrink the
local file to product residue, then delete. Two extractions already shipped
this way (`NoProgressTracker` → 1.5.0, MicrocompactMiddleware → `ac73382`,
upstream PR pending on `feat/microcompact-middleware`).

Ordered by value (generic-LOC reclaimed locally + capability gained by the
crate). Figures are non-test lines; tests roughly double each.

## 1. Tool-call dialect layer (~1,900) — `pformat.rs` + `dispatcher.rs` + `harness/parse.rs`

- Crate gains: a `ToolCallDialect` trait beside native tool calling —
  P-Format compact positional calls (`name[a|b]`, ~80% token cut on
  tool-heavy turns — a genuine crate selling point for small/local models
  that lack native tool calling), permissive XML/JSON parsing with arg-key
  drift recovery (`TOOL_ARG_KEYS`: arguments|args|parameters|params|input).
- Product residue: none identified — zero DomainEvent coupling.
- Unlock: old-plan C1 step 4 becomes a pure local delete once no live path
  parses provider text (transcript compat reads move with it).

## 2. Multimodal attachment resolver (~1,550 of 1,690) — `multimodal.rs`

- Crate gains: marker → provider content-block resolution, mime allowlist,
  PDF text extraction, fetch gating, truncation budgets.
- Product residue: the `[IMAGE:]`/`[FILE:]` marker convention itself.
- Note: land after doc 02 so thinking/reasoning block handling doesn't need
  to be ported twice.

## 3. Overflow-to-artifact tool-result store (~500) — `tool_result_artifacts/`

- Already built on crate `Store`; overflow-to-artifact is a generic harness
  concern (pairs with the crate's microcompact). Residue: PII scrub hook.
- Include `subagent_runner/extract_tool.rs` (612) + `handoff.rs` (287):
  progressive-disclosure Q&A over handoff-cached oversized results — the
  natural companion API.

## 4. Durable TaskStore upgrade + detached lifecycle (~crate work enabling a 1,631-line local delete)

- Crate gains: SQLite-backed `TaskStore` (behind the existing `sqlite`
  feature) with lifecycle history, cancellation records, wait/kill/steer
  handles, replay/listing by parent/root/thread (goal.md §4/§11).
- Unlock: `agent_orchestration/running_subagents.rs` 1931 → ≤300 (old-plan
  C6.2) stops being an adapter squeeze and becomes a projection.

## 5. Hook trait machinery (~450) — `hooks.rs` + `stop_hooks.rs`

- Crate gains: `PostTurnHook`/`StopHook` traits + scheduling shell (incl.
  the archivist's ~200-line hook-scheduling shell). Product hook bodies
  (archivist, memory, cost stop-hooks) stay local.

## 6. Host runtime adapter core (~350) — `host_runtime.rs`

- Native/Docker `RuntimeAdapter` overlaps crate `WorkspaceIsolation`;
  merge as backends of one crate abstraction.

## 7. Fuzzy tool ranker (~250) — `tool_filter.rs`

- Generic ranking into `ContextualToolSelectionMiddleware` (whose shadow
  flip is old-plan C3.1); Composio input types stay product.

## 8. LLM triage node (~800 of 3,779) — `triage/` evaluator core

- Crate gains: a generic "classify with tiered model fallback + cache +
  verdict parse" graph node. Envelope/escalation/events (Composio,
  DomainEvents, agent ids) stay product.

## 9. Small fry

- `ArgRecoveryMiddleware` core (~150).
- Task-local context carriers (`fork_context`, `sandbox_context`,
  `task_recency_context`, `turn_attachments_context`, ~500): not an
  extraction — replace with typed fields on the crate execution context
  (`ToolExecutionContext`/`RunContext`), old-plan C6.4. Requires a crate API
  addition (generic `extensions: TypeMap` on the run context is the clean
  form) — do that API change first, then delete all four locally.

## Workflow per extraction (checklist)

1. Branch in `vendor/tinyagents` (`feat/<name>`); port code + tests to crate
   idioms (types.rs/test.rs split, ≤500-line files, module README).
2. `cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test`
   in the submodule.
3. Bump gitlink in OpenHuman on a paired branch; swap local callers to the
   crate type; shrink local file to residue (or delete); run OpenHuman
   suites per the two-branch CI model.
4. Tick `99-deletion-ledger.md` in the old plan folder (it remains the
   master ledger) and note the extraction here.
5. Push submodule branch to `tinyhumansai/tinyagents`, open upstream PR.
   Local gitlink may run ahead of upstream merge (already the case for
   `ac73382`) — keep the queue shallow: ≤2 unmerged submodule branches.

## License note

tinyagents is GPL-3.0-only; OpenHuman consumes it in-tree already, so
extraction changes nothing legally — but extracted code becomes GPL. Flag
any file with third-party-licensed snippets before moving (none known).
