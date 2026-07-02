# 01.4 — Tool output budgets, summarizer, artifacts

Finish moving tool-result post-processing to `after_tool` middleware and
delete the legacy hooks.

## Steps

Current status: TinyAgents `ToolOutputMiddleware` already runs as `after_tool`
for payload summarization, TokenJuice `compact_output_with_policy`, per-tool
policy-derived output caps, generic byte budgets, and action-workspace artifact
spill for oversized session tool results. Payload summarization and TokenJuice
shrinks now emit TinyAgents `AgentEvent::Compressed`, and persisted
action-workspace artifacts are indexed in `RunContext.stores` under
`openhuman_tool_result_artifacts` while the existing `.txt` file and `file_read`
envelope remain the source of truth. Remaining work is to remove the legacy
executor hooks once the old path is gone and move the summarizer child dispatch
onto `SubAgent::invoke_in_parent`. The live TinyAgents middleware already calls
the parent-context-aware `PayloadSummarizer::maybe_summarize_in_parent`, and
that implementation now dispatches the summarizer through
`SubAgent::invoke_in_parent` so child lineage/events inherit the parent
TinyAgents context. The legacy direct executor still uses the older
`run_subagent` dispatch path until that executor is removed. The stale
default-Full `tokenjuice::compact_tool_output` wrapper is removed; TinyAgents
and the legacy direct executor use the policy-aware entry point.

1. `payload_summarizer.rs` (490 lines, oversized-result compression via a
   `summarizer` sub-agent + circuit breaker): `ToolOutputMiddleware` now calls
   the parent-context-aware summarizer seam, and that live path uses
   `SubAgent::invoke_in_parent` for child depth/event lineage. Remaining work:
   delete the older direct-executor dispatch path and emit any additional
   `SummaryRecord`-style provenance needed beyond `AgentEvent::Compressed`.
2. `tokenjuice::compact_tool_output`: deleted after confirming
   `compact_output_with_policy` covers the live TinyAgents middleware and
   legacy direct executor paths; decision recorded in
   `src/openhuman/tokenjuice/README.md`.
3. `harness/tool_result_artifacts/mod.rs` (476 lines, artifact spill):
   keep spill policy and action-workspace `.txt` writes in OpenHuman, but keep
   the run's `StoreRegistry` (`RunContext.stores`) populated with structured
   artifact metadata so replay can find the model-facing preview's full body.
4. Collapse the per-call executor chain in
   `session/agent_tool_exec.rs` (505 lines): visibility gate → middleware
   (01.3), permission/approval → middleware (already), byte budget/summarizer
   → this step. What remains is session bookkeeping; fold it into the
   session shell.

## Deletions

- `src/openhuman/agent/harness/payload_summarizer.rs`.
- Byte-budget/summarizer branches of `session/agent_tool_exec.rs`.
- `tokenjuice::compact_tool_output`.

## Acceptance

- Oversized tool result → compressed with provenance event; parity fixture
  vs old summarizer output shape.
- No tool-result post-processing outside middleware.
