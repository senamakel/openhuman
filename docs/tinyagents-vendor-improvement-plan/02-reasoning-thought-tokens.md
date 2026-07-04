# 02 — Thought Tokens End-to-End (vendor crate work)

Goal: reasoning/thinking content becomes a first-class, streamed, persisted,
replayed, and priced citizen of the crate — so OpenHuman deletes
`ThinkingForwarder` and stops re-projecting thinking via its own
`ProviderDelta::ThinkingDelta` bridge (`session/tool_progress.rs:226`).

All steps are vendor-crate changes (`vendor/tinyagents`), committed on a
submodule feature branch, gitlink-bumped into OpenHuman, PR'd upstream.
Precedent: `NoProgressTracker` (#7), MicrocompactMiddleware (`ac73382`).

## Step 1 — Message representation

- Add `ContentBlock::Thinking { text: String, signature: Option<String> }`
  and `ContentBlock::RedactedThinking { data: String }` to
  `harness/message/types.rs:21-30`.
- Serde: additively tagged; existing transcripts (no thinking blocks) parse
  unchanged. Round-trip tests in `message/test.rs`.
- `AssistantMessage` rendering helpers must skip thinking blocks for
  plain-text extraction (`Message::text()`-style accessors) but preserve them
  for provider replay.

## Step 2 — Accumulator persistence

- `StreamAccumulator::finish` (`harness/model/mod.rs:683-710`): emit the
  accumulated `self.reasoning` as a leading `ContentBlock::Thinking` on the
  final message instead of dropping it. Carry signature fragments (Anthropic
  `signature_delta`) through a new accumulator field.
- Keep the existing `reasoning()` side-channel accessor for backward compat.

## Step 3 — Delta plumbing

- Populate `MessageDelta.reasoning` from providers:
  - OpenAI path: parse `delta.reasoning_content` (DeepSeek/compat) and
    o-series reasoning summaries instead of hardcoding `String::new()`
    (`providers/openai/mod.rs:772`); add the wire fields to
    `openai/types.rs`.
  - Anthropic path (05): `thinking_delta` / `signature_delta` events.
- Thread reasoning into the middleware-facing `ModelDelta`
  (`agent_loop/mod.rs:980-984`) — closes sdk-gaps §3. Add
  `ToolDelta.tool_name` on the first fragment (tool-name-on-start), the
  other half of that gap.
- Emit reasoning in `AgentEvent::ModelDelta` with parent/root run
  attribution (pairs with doc 03's sub-agent propagation so
  `SubagentThinkingDelta` can become a projection).

## Step 4 — Replay correctness (Anthropic contract)

Anthropic requires thinking blocks (with signatures) to be replayed verbatim
in the assistant turn preceding tool results. With Step 1 the blocks live in
the transcript; the provider translation (05) must:

- Serialize `Thinking`/`RedactedThinking` blocks back onto the wire for
  assistant messages in multi-turn tool-use conversations.
- Never send thinking blocks to providers that reject them (OpenAI compat
  path strips them) — provider capability flag `supports_thinking` on
  `ModelProfile`.
- Property test: build a 3-turn tool-use conversation with thinking, assert
  byte-stable signature replay.

## Step 5 — Usage & cost

- Add `completion_tokens_details.reasoning_tokens` to the OpenAI wire struct
  (`openai/types.rs:235-256`) and map it in `convert_usage`
  (`openai/mod.rs:708-719`).
- Anthropic (05): thinking output tokens are billed as output; keep
  `reasoning_tokens` as the *reported* subset where the API exposes it.
- `CostTotals.reasoning_cost` (`cost/mod.rs:69`) then prices real numbers —
  feeds OpenHuman's C3 budget-flip criteria (pricing table wired).

## Step 6 — OpenHuman follow-through (after gitlink bump)

1. Delete `ThinkingForwarder` (old-plan C7 item; sdk-gaps §3 closes).
2. `AgentProgress::ThinkingDelta`/`SubagentThinkingDelta` become projections
   of crate `AgentEvent::ModelDelta.reasoning` — shrinks
   `session/tool_progress.rs` toward deletion (with doc 03).
3. Persisted-transcript compat: OpenHuman's `multimodal.rs` reasoning-block
   handling shrinks once the crate owns thinking blocks in messages.

## Tests (crate)

- Provider-level: SSE fixture streams with reasoning deltas → accumulator →
  message contains `Thinking` block; usage carries reasoning tokens.
- Loop-level: middleware `on_model_delta` sees reasoning; `AgentRun.messages`
  persists it; replay serialization per provider capability.
- Extend `tests/e2e_reasoning_and_selection.rs` beyond hand-fed deltas.

Effort: **L** (1–2 weeks) crate-side; OpenHuman follow-through **S**.
