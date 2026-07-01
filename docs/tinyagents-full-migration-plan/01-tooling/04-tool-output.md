# 01.4 — Tool output budgets, summarizer, artifacts

Finish moving tool-result post-processing to `after_tool` middleware and
delete the legacy hooks.

## Steps

1. `payload_summarizer.rs` (490 lines, oversized-result compression via a
   `summarizer` sub-agent + circuit breaker): re-express as an `after_tool`
   stage inside `ToolOutputMiddleware` (byte-cap already there). Use
   `SubAgent::invoke_in_parent` for the summarizer child so usage/lineage
   roll up natively. Emit `SummaryRecord`-style provenance via
   `AgentEvent::Compressed`.
2. Re-wire `tokenjuice::compact_tool_output` (currently has NO production
   caller — finding from the doc sweep) as an optional semantic-compaction
   stage in the same middleware, or delete it if the summarizer stage
   covers it. Decide once, record in `src/openhuman/tokenjuice/README.md`.
3. `harness/tool_result_artifacts/mod.rs` (476 lines, artifact spill):
   keep spill policy (product), but write artifacts through the run's
   `StoreRegistry` (`RunContext.stores`) so replay can find them.
4. Collapse the per-call executor chain in
   `session/agent_tool_exec.rs` (505 lines): visibility gate → middleware
   (01.3), permission/approval → middleware (already), byte budget/summarizer
   → this step. What remains is session bookkeeping; fold it into the
   session shell.

## Deletions

- `src/openhuman/agent/harness/payload_summarizer.rs`.
- Byte-budget/summarizer branches of `session/agent_tool_exec.rs`.
- `tokenjuice::compact_tool_output` if superseded (call-site search first).

## Acceptance

- Oversized tool result → compressed with provenance event; parity fixture
  vs old summarizer output shape.
- No tool-result post-processing outside middleware.
