# 04 — Performance (vendor crate work)

Goal: remove O(history) per-turn overhead from the agent loop. OpenHuman
transcripts are long (multi-hundred-message sessions with large tool
results); every listed cost below scales with transcript size and is paid
one or more times **per model call**.

## 1. Stop cloning the transcript per turn/attempt — biggest win

Today:

- `ModelRequest::new(messages.clone())` per iteration
  (`agent_loop/mod.rs:356`).
- `model.invoke(state, request.clone())` / `model.stream(state,
  request.clone())` per retry/fallback **attempt** (`:804`, `:937`) — a
  3-model fallback chain with 2 retries can clone the full history 6×.

Plan:

- Change `ModelRequest.messages` to `Arc<Vec<Message>>` (or `Arc<[Message]>`)
  with copy-on-write semantics (`Arc::make_mut` on the rare mutation paths —
  middleware that rewrites history). Public builder API stays source-
  compatible via `impl Into<Arc<Vec<Message>>>`.
- Retry/fallback passes `&ModelRequest`; `ChatModel::invoke/stream` take
  `&ModelRequest` (breaking trait change — acceptable pre-2.0, coordinated
  with OpenHuman's adapter in the same gitlink bump).
- The loop appends assistant/tool messages to its own `Vec` and rebuilds the
  `Arc` view per turn (one cheap `Arc::new` of a `Vec` it already owns, or
  an immutable persistent-list structure if profiling justifies it — start
  with `Arc<Vec>`, measure).

## 2. Cache tool schemas per run

`.with_tools(self.tools.schemas())` rebuilds every `ToolSchema` (cloning
name/description/`parameters: Value`) each iteration (`agent_loop/mod.rs:356`).
Tool sets are fixed for a run (dynamic-selection middleware operates on the
request afterward). Plan: compute `Arc<Vec<ToolSchema>>` once at run start;
middleware that filters tools clones only then (copy-on-filter).

## 3. Response-cache key without full serde round-trips

`cache_key` = `serde_json::to_value(request)` → canonicalize → `to_vec` →
SHA-256 over the entire request per cached call (`cache/mod.rs:101-106`).
Plan: maintain an incremental hash — hash each message once when appended
(messages are immutable after append), fold message digests + tool-schema
digest + params digest into the key. Falls out naturally once messages are
`Arc`'d (attach a lazily-computed digest per message).

## 4. Parallel tool execution

Tools run strictly serially (`agent_loop/mod.rs:526-659`) while
`parallel_tool_calls` exists only as a capability flag. Plan:

- Execute a turn's tool calls via `futures::stream::iter(...)
  .buffer_unordered(cap)` but **emit results in request order** (index-sorted
  reassembly) so transcripts stay deterministic.
- Default cap: 1 (today's behavior) — opt-in via
  `AgentHarnessBuilder::with_tool_concurrency(n)` and a per-tool
  `ToolPolicy::serial` escape hatch (side-effecting tools opt out).
- Middleware contract: `before_tool`/`after_tool` hooks fire per call; wrap
  middleware must be `Send + Sync` (already is). Events carry call ids so
  interleaving is attributable (pairs with doc 03 lifecycle events).
- OpenHuman: keep cap=1 initially; enable for read-only tool families after
  the approval-gate interaction is reviewed (approval middleware must park
  the whole batch, not one call).

## 5. Provider translation

`translate_request` re-clones all messages/tool schemas and re-serializes
tool args per call (`openai/mod.rs:404-449, 570-584`). Plan: with #1/#2 the
inputs are `Arc`'d; add a per-run memo of the translated static prefix
(system + tools JSON) keyed by the schema digest from #3. This also makes
prefix-stability for provider prompt caches (05) explicit rather than
incidental.

## 6. Minor

- `StreamAccumulator::push_tool_chunk` linear scan (`model/mod.rs:643-650`)
  → index map by `call_id` (only matters for many-tool turns; cheap fix).
- `InMemoryResponseCache` mutex per get/put (`cache/mod.rs:119-131`) — fine;
  re-check only if #3 makes cache use hot.

## Measurement (gate for merging #1–#3)

Add a criterion-style bench (or a `live_*`-excluded integration bench) in the
crate: synthetic 500-message transcript, 40 tools, 20-turn loop against
`MockModel` — assert allocations/turn and wall time before/after. OpenHuman
side: `[budget_shadow]`-style log of per-turn overhead in the adapter for one
release to confirm on real sessions.

Effort: #1 **M** (trait-breaking, coordinated bump), #2 **S**, #3 **S–M**,
#4 **M**, #5 **S**. Order: #2/#3 first (non-breaking), then #1 (+#5), #4
independent.
