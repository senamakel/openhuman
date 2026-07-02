# Stage 5 — Subconscious steering loop (offline reflection)

## Goal command

> Extend the existing **`SubconsciousEngine`** so its cron/heartbeat tick consumes the
> orchestration layer's **compressed history** and **cumulative world-state diff** (stage 4) and
> emits short, dense **steering directives** that are injected into the Reasoning orchestrator's
> prompts on subsequent cycles. The subconscious stays fully offline: no channels, no user
> contact, no tools with external effects — its only output is the directive (and its existing
> proactive-notify path, unchanged).

## Read first

- `src/openhuman/subconscious/README.md`, `engine.rs` (three-stage tick), `heartbeat/mod.rs`,
  `store.rs`, `agent/{agent.toml, prompt.md, graph.rs}`, `global.rs`.
- `src/openhuman/scheduler_gate/` — capacity gating already applied to ticks.
- Stage 4's store tables (`compressed_history`, `world_diff`) and `orchestration/types.rs`.
- `docs/arch-subconscious.md` §3.2 — world-diff evaluation semantics ("macro-trends, filter
  localized variance").

## Deliverables

1. **New tick stage `orchestration_review`** in `SubconsciousEngine::tick_inner`, after
   `memory_diff` / before `decide` (or folded into `prepare_context` — decide against the existing
   stage contract and document): loads, since the last reviewed cursor,
   - unreviewed `compressed_history` rows (bounded batch, oldest-first), and
   - the **full world-diff timeline** (it is the cumulative object the spec requires — cap by
     summarizing the oldest ranges through the same 20:1 hook if it outgrows the budget).
2. **Steering synthesis**: extend the slim subconscious agent prompt with a steering section —
   output contract: at most ~150 tokens, imperative, model-agnostic
   (`STEERING_DIRECTIVE: …` semantics from the spec), plus a machine field
   `expires_after_cycles: u32` (default 20). Runs on the existing `hint:subconscious` route with
   `SubconsciousTainted` origin.
3. **Steering store** (`orchestration/store.rs`, table `steering_directives`): append-only history
   `{ id, text, created_at, source_tick_id, expires_after_cycles, superseded_by }`; "current
   directive" = latest non-expired. The unified graph's `execute` node loads it into
   `state.subconscious_steering` at cycle start — the spec's out-of-band
   `update_state(as_node="subconscious_cron")` pattern: the subconscious is a decoupled writer
   into the checkpointed thread, never an edge in the wake graph.
4. **Feedback provenance**: each directive records which compressed-history rows / diff seq range
   it was derived from; reviewed cursor advances only on successful persist (idempotent ticks).
5. **Subconscious chat surface**: publish each emitted directive as an
   `OrchestrationMessage { chat_kind: Subconscious }` (and optionally mirror it as a self-DM over
   tiny.place so it is visible in other clients) — this is what fills the pinned "Subconscious"
   window in the UI (stage 6).

## Tasks

1. Add the tick stage + cursor kv; no-op cleanly when orchestration is disabled or tables empty.
2. Prompt work: steering section with 2–3 few-shot examples; parse/validate the structured output
   (reject and retry once on contract violation; skip tick on second failure, log warn).
3. Store + supersede/expiry logic; unit tests (expiry by cycle count, supersede chain).
4. Integration test: seed fake compressed rows + diff timeline → tick → directive persisted →
   stage 4 `apply_steering` picks it up on the next cycle (assert it lands in the system prompt of
   the mock provider call).
5. Isolation test: assert the tick's tool surface contains no channel/effect tools and that no
   tinyplace *outbound* op other than the optional self-DM mirror is reachable.

## Acceptance criteria

- A heartbeat tick over seeded orchestration data produces exactly one current directive; the next
  reasoning cycle demonstrably runs with it injected.
- Ticks are idempotent (re-run without new data → no new directive) and cheap when idle.
- Directives appear in the Subconscious chat window feed (stage 6 consumes them).
- Existing subconscious behaviors (memory diff, planner, notify_user) unchanged — their tests stay
  green.
