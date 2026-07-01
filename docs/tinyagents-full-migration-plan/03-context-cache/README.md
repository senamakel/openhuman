# 03 — Context management & caching

Live context reduction already runs on `ContextCompressionMiddleware` +
`MessageTrimMiddleware` + `tinyagents/summarize.rs`, and microcompact/
cache-align exist as middlewares. This workstream deletes the superseded
originals and adopts the crate cache layer.

Target SDK surface: `ResponseCache`/`InMemoryResponseCache` + `cache_key`,
`CachePolicy { response_cache_enabled, protect_prompt_prefix }`,
`PromptCacheLayout` + `PromptCacheGuardMiddleware` + `CacheLayoutEvent`,
`AgentEvent::{CacheHit, CacheMiss, Compressed}`, `SummaryRecord`,
`ModelRequest::cache_segments(Vec<PromptSegment>)`.

Steps:

1. `01-delete-legacy-context.md` — remove dead reducers, consolidate stats.
2. `02-cache-layer.md` — response cache + prompt-prefix protection.

Done when: `context/` contains only OpenHuman-specific state (prompt
assembly inputs, session-memory bookkeeping, stats projection) and cache
correctness is asserted by crate events, not warn-logs.

Keep (product): `context/manager.rs` prompt-assembly surface,
`session_memory.rs`, `channels_prompt.rs`, `context/stats` for the UI
utilisation footer.
