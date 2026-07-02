# 03.1 — Delete superseded context reducers

`tinyagents/middleware.rs` doc-comments already note these were "effectively
dead on the live loop" before being re-expressed as middlewares.

Current status: `harness/compaction/cache_align.rs` and the
`harness/compaction/mod.rs` shim are deleted; the volatile-token detector now
lives inside TinyAgents `CacheAlignMiddleware`.

## Steps

1. `context/microcompact.rs` (269): deleted. The shared placeholder/default
   constants now live in `context/mod.rs`, and the live clearing logic is the
   TinyAgents `MicrocompactMiddleware`.
2. `context/pipeline.rs` (454) + `context/guard.rs` (236): superseded by
   `ContextCompressionMiddleware` + `summarize.rs` (0.90 threshold mirrored
   as `SUMMARIZE_THRESHOLD_FRACTION`). They survive only as the data model
   behind `ContextManager::stats()` — extract the minimal stats structs into
   `context/stats.rs`, delete the reduction machinery.
3. `harness/compaction/cache_align.rs` (200) + `compaction/mod.rs` shim:
   superseded by `CacheAlignMiddleware`; directory deleted.
4. `context/tool_result_budget.rs` (172): deleted. The UTF-8-safe fallback
   truncation helper now lives beside action-workspace artifact preview
   handling; the live TinyAgents path uses `ToolOutputMiddleware` per-tool
   caps.
5. Summarization provenance: ensure every compression emits
   `AgentEvent::Compressed` with `SummaryRecord` data (source ids,
   before/after token estimates) — wire `ContextCompressionMiddleware::
   records()` into the run outcome/journal.

## Deletions

- `context/pipeline.rs`, `context/guard.rs` (post stats-extraction).
  `context/microcompact.rs` and `harness/compaction/` are already deleted.

## Acceptance

- `context/` line count drops ~1.2k; `stats()` UI footer parity test green;
  compression fires at 90% window on all three turn paths (existing
  summarize tests).
