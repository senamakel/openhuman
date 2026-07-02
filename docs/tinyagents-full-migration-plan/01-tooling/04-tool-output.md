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
envelope remain the source of truth. The live TinyAgents middleware already
calls the parent-context-aware `PayloadSummarizer::maybe_summarize_in_parent`,
and that implementation dispatches the summarizer through
`SubAgent::invoke_in_parent` so child lineage/events inherit the parent
TinyAgents context. The older direct-executor payload-summarizer hook is
removed, `session/agent_tool_exec.rs` is now a test-only parity shim, and the
stale default-Full `tokenjuice::compact_tool_output` wrapper is removed.

1. `payload_summarizer.rs` (live in `src/openhuman/tinyagents/`; oversized-result
   compression via a `summarizer` sub-agent + circuit breaker):
   `ToolOutputMiddleware` now calls the parent-context-aware summarizer seam,
   and that live path uses `SubAgent::invoke_in_parent` for child depth/event
   lineage. Remaining work: emit any additional `SummaryRecord`-style
   provenance needed beyond `AgentEvent::Compressed`.
2. `tokenjuice::compact_tool_output`: deleted after confirming
   `compact_output_with_policy` covers the live TinyAgents middleware and
   legacy direct executor paths; decision recorded in
   `src/openhuman/tokenjuice/README.md`.
3. `harness/tool_result_artifacts/mod.rs` (588 lines, artifact spill):
   keep spill policy and action-workspace `.txt` writes in OpenHuman, but keep
   the run's `StoreRegistry` (`RunContext.stores`) populated with structured
   artifact metadata so replay can find the model-facing preview's full body.
4. `session/agent_tool_exec.rs` is no longer production-compiled; it remains
   behind `cfg(test)` so the old direct-executor parity tests can be migrated or
   deleted deliberately.

## Deletions

- Legacy direct-executor tests that depend on `session/agent_tool_exec.rs`.
- `tokenjuice::compact_tool_output`.

## Acceptance

- Oversized tool result → compressed with provenance event; parity fixture
  vs old summarizer output shape.
- No tool-result post-processing outside middleware.
