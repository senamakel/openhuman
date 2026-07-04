# 01 — TinyAgents Crate Audit (vendor/tinyagents @ 1.5.0 + ac73382)

~41.5k non-test LOC (~60k with tests); 54 integration tests, 15 examples.
Deps: tokio (sync/time/macros), reqwest (rustls/json/stream), serde, sha2;
optional `rusqlite` (`sqlite`), `rhai` (`repl`). No provider-specific SDKs.

The crate's own `goal.md` is a pre-written gap list from OpenHuman's
migration perspective and corroborates most findings below.

## 1. Module map

- `harness/` — the agent runtime. Largest: `middleware/` (~5.3k, trait +
  ~18 built-ins incl. retry/timeout/fallback/budget/tool-policy/contextual
  selection/approval/redaction/prompt-cache guard), `providers/` (~3k:
  `MockModel` + one real `OpenAiModel`), `agent_loop/` (~2.3k), `model/`
  (~1.9k: `ChatModel`, `ModelRequest/Response`, `ModelStream`,
  `StreamAccumulator`, `ModelRegistry`), `observability/` (~1.8k journals/
  status/Langfuse), `subagent/` (~1.2k), `events/` (~1.2k), plus store/tool/
  summarization/embeddings/steering/cache/usage/cost/no_progress/testkit.
- `graph/` (~19k) — durable typed graphs: compiled runtime, checkpoints
  (memory/file/sqlite), orchestration `TaskStore` + tools, `todos`, `goals`,
  `map_reduce`, subagent/subgraph nodes, recursion, export, testkit.
- `registry/`, `language/` (.rag), `repl/` (.ragsh, feature-gated).
- Ergonomics gap: graph surface is fully re-exported at crate root, but core
  harness types (`AgentHarness`, `ChatModel`, `ModelRequest`, `OpenAiModel`,
  middleware, message types) require deep `harness::…` paths (`lib.rs`).

## 2. The harness loop (what's good)

`AgentHarness::run_loop` (`harness/agent_loop/mod.rs:293-673`): cancellation
checkpoint → steering drain → deadline/call caps → build request →
`before_model` → wrap-onion model call with retry + fallback chain
(`:766-859`) + per-call `tokio::time::timeout` budget (`:902-917`) + response
cache (`:685-756`) → `after_model` → usage/events → `MiddlewareControl`
(StopWithFinal/Interrupt) → tools → repeat. Unknown-tool policy
`Fail | ReturnToolError | Rewrite` (`:563-618`). Sub-agents nest as tools
with a depth cap. Rich extension points; the loop *shape* itself is fixed.

## 3. Defects & gaps (file:line), grouped

### Reasoning / thought tokens — scaffolding exists, last mile unwired

- `ContentBlock` = `Text | Json | Image | ProviderExtension`
  (`harness/message/types.rs:21-30`) — **no Thinking/Redacted variant**; no
  persisted home for Anthropic thinking blocks or their signatures.
- OpenAI SSE path hardcodes `reasoning: String::new()` per delta
  (`providers/openai/mod.rs:772`); never parses `reasoning_content` or
  o-series reasoning.
- `StreamAccumulator::finish` **drops accumulated reasoning** — final message
  built from text + tool chunks only (`model/mod.rs:683-710`).
- Loop's middleware-facing `ModelDelta` built with content + tool_call only,
  dropping reasoning (`agent_loop/mod.rs:980-984`) — the exact sdk-gaps §3
  item blocking `ThinkingForwarder` deletion.
- `Usage.reasoning_tokens` (`usage/types.rs:29`) and
  `CostTotals.reasoning_cost` (`cost/mod.rs:69`) exist but are **always 0**:
  `convert_usage` maps only prompt/completion/total/cached
  (`openai/mod.rs:708-719`); `completion_tokens_details.reasoning_tokens`
  isn't even in the wire struct (`openai/types.rs:235-256`).
- No signature/redacted_thinking handling anywhere → correct multi-turn
  extended-thinking + tool-use vs native Anthropic is currently impossible.

### Streaming — real SSE, but observation-only and single-level

- OpenAI streaming is genuine incremental SSE (`openai/mod.rs:1020-1073`,
  line buffering `:874-896`, chunk folding `:753-804`); tool-call arg
  fragments stream as `ToolCallDelta` (`:796-799`).
- **`invoke_streaming` returns a completed `AgentRun`, not a `Stream`**
  (`agent_loop/mod.rs:195-241`); deltas reachable only via `EventSink`/
  middleware (`:929-994`).
- **Sub-agents never stream**: `SubAgentTool::call` →
  `SubAgent::invoke` non-streaming path (`subagent/mod.rs:535`, `:179-189`).
  No parent/root attribution on deltas.
- No explicit tool-call started/completed stream events.
- `MockModel::stream` replays a completed `invoke` (`model/types.rs:536-549`)
  — mock-backed tests aren't truly incremental.

### Performance — correctness-first cloning in the hot loop

- Full history cloned per turn: `ModelRequest::new(messages.clone())`
  (`agent_loop/mod.rs:356`); full request cloned again **per retry/fallback
  attempt** (`:804`, `:937`).
- Tool schemas rebuilt every iteration: `.with_tools(self.tools.schemas())`
  (`:356`).
- Provider `translate_request` re-clones every message + tool schema and
  re-serializes tool args per call (`openai/mod.rs:404-449, 570-584`).
- Response-cache key = serde round-trip + canonicalize + SHA-256 over the
  **entire request** per cached call (`cache/mod.rs:101-106`).
- `StreamAccumulator::push_tool_chunk` linear scan per fragment
  (`model/mod.rs:643-650`).
- Tools execute **strictly serially** (`agent_loop/mod.rs:526-659`);
  `parallel_tool_calls` is a capability flag only.

### Providers — one adapter, many presets

- Only `OpenAiModel` is real; `anthropic()`/`deepseek`/`groq`/`xai`/
  `openrouter`/`together`/`mistral`/`ollama` are base-URL presets on the
  Chat Completions wire (`openai/mod.rs:309-383`).
- **No Anthropic Messages API**: zero `cache_control`/`ephemeral` hits in
  providers; no thinking config; `Usage.cache_creation_tokens` always 0
  (only `cache_read_tokens` from `prompt_tokens_details.cached_tokens`,
  `openai/mod.rs:713-716`).
- Prompt-cache shaping is metadata-only: `cache_segments` /
  `prompt_fingerprint` / `cacheable_prefix_ids()`
  (`model/types.rs:385-405`) + `PromptCacheGuardMiddleware`
  (`middleware/mod.rs:19-60`) never reach any wire field — the spec's
  "extreme prompt caching" (`docs/spec/README.md:258-278`) is observability,
  not request shaping.
- Spec explicitly plans feature-flagged native `openai`/`anthropic`/`ollama`
  (`docs/spec/README.md:279-284`; slot documented at
  `providers/types.rs:322-337`).

### Durability / storage

- `TaskStore` is in-memory/JSONL only; no lifecycle history/replay
  (goal.md §4/§11) — what `running_subagents.rs` (1931 LOC) needs to shrink.
- SQLite ownership: bundled `rusqlite 0.40` vs app-owned sqlite → needs a
  trait-first/connection-adapter path (goal.md §5).
- `Store` has no compare-and-set (single-writer constraint documented in the
  old plan).

## 4. Test coverage holes

No tests for: native Anthropic/prompt caching/thinking (no impl), reasoning
end-to-end (only hand-fed accumulator test,
`tests/e2e_reasoning_and_selection.rs:57-98`), parallel tool execution,
caller-consumable streaming, sub-agent stream propagation. Live-provider
tests exist behind env keys; conformance suites exist for graph/checkpoint.

## 5. Ranked improvement opportunities (effort: S≈1-2d, M≈3-5d, L≈1-2w, XL multi-week)

1. **Reasoning end-to-end** — L — doc 02. Unblocks `ThinkingForwarder`
   deletion + real reasoning cost accounting.
2. **Hot-loop de-cloning + cache-key + schema caching** — M — doc 04.
   Biggest runtime win for OpenHuman's long transcripts.
3. **Streaming out of the harness + up from sub-agents** — L — doc 03.
   Unblocks `tool_progress.rs`/progress projection deletions (C4).
4. **Native Anthropic Messages provider + cache_control shaping** — L→XL —
   doc 05. Highest-leverage provider gap.
5. **Parallel tool execution** — M — doc 04 §4.
6. **Usage completeness (OpenAI reasoning tokens) + catalog pricing** — S→M
   — doc 02 §5; feeds old-plan C3 budget flip.
7. **Durable TaskStore (SQLite/JSONL lifecycle + replay)** — L — doc 06 §4.
8. **SQLite dependency flexibility** — M — goal.md §5.
9. **Crate-root re-export ergonomics** — S.
