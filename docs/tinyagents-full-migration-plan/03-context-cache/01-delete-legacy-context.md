# 03.1 — Delete superseded context reducers

`tinyagents/middleware.rs` doc-comments already note these were "effectively
dead on the live loop" before being re-expressed as middlewares.

Current status: `harness/compaction/cache_align.rs` and the
`harness/compaction/mod.rs` shim are deleted; the volatile-token detector now
lives inside TinyAgents `CacheAlignMiddleware`.

## Steps

1. `context/microcompact.rs` (269): superseded by `MicrocompactMiddleware`.
   Call-site search; move any constant/policy the middleware still imports;
   delete file + re-point tests.
2. `context/pipeline.rs` (454) + `context/guard.rs` (236): superseded by
   `ContextCompressionMiddleware` + `summarize.rs` (0.90 threshold mirrored
   as `SUMMARIZE_THRESHOLD_FRACTION`). They survive only as the data model
   behind `ContextManager::stats()` — extract the minimal stats structs into
   `context/stats.rs`, delete the reduction machinery.
3. `harness/compaction/cache_align.rs` (200) + `compaction/mod.rs` shim:
   superseded by `CacheAlignMiddleware`; directory deleted.
4. `context/tool_result_budget.rs` (172): superseded by
   `ToolOutputMiddleware` per-tool caps — verify no remaining caller
   (the `agent_tool_exec` chain shrink in 01.4 removes the last), delete.
5. Summarization provenance: ensure every compression emits
   `AgentEvent::Compressed` with `SummaryRecord` data (source ids,
   before/after token estimates) — wire `ContextCompressionMiddleware::
   records()` into the run outcome/journal.

## Deletions

- `context/microcompact.rs`, `context/pipeline.rs`, `context/guard.rs`
  (post stats-extraction), `context/tool_result_budget.rs`,
  `harness/compaction/` (whole dir).

## Acceptance

- `context/` line count drops ~1.2k; `stats()` UI footer parity test green;
  compression fires at 90% window on all three turn paths (existing
  summarize tests).
