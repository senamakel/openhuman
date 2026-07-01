# 01.2 — Unknown-tool recovery via RunPolicy

Crate `RunPolicy.unknown_tool` now has `UnknownToolPolicy::{Fail,
ReturnToolError, Rewrite { tool_name }}` plus an `AgentEvent::UnknownToolCall`
variant — the sentinel is deletable.

## Steps

1. In `assemble_turn_harness` set `RunPolicy.unknown_tool =
   UnknownToolPolicy::ReturnToolError` (preserves the legacy "model corrects
   itself" behavior with the original requested name).
2. Preserve wording differences: the current sentinel handler emits different
   text for sub-agent vs top-level. If `ReturnToolError`'s message is not
   customizable, keep a thin `before_tool` middleware ONLY for wording, or
   use `Rewrite { tool_name }` pointed at a minimal handler — decide by
   reading the crate's error text; prefer deleting all custom code.
3. Verify `AgentEvent::UnknownToolCall` reaches `OpenhumanEventBridge` and is
   projected distinctly from "tool executed and failed".

## Deletions

- `UNKNOWN_TOOL_SENTINEL` + sentinel tool registration in
  `src/openhuman/tinyagents/tools.rs`.
- `UnknownToolRewriteMiddleware` in `src/openhuman/tinyagents/middleware.rs`
  (+ its tests, rewritten as RunPolicy behavior tests).

## Acceptance

- Hallucinated tool name → recoverable tool error, run continues; event
  stream records the original requested name; no sentinel appears in
  transcripts or advertised schemas.
- Sub-agent and top-level wording parity tests green.
