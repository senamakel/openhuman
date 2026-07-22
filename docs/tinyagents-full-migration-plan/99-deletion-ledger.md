# TinyAgents migration deletion ledger

This ledger records host-side removals required by
[`tinyagents-migration-plan-2026-07-22.md`](../tinyagents-migration-plan-2026-07-22.md).
A row moves to `DELETED` only after its crate-backed replacement and the named
parity evidence are in place. Generic code moved into `vendor/tinyagents` must
also name its upstream PR before the host copy is removed.

| Work package | Host artifact | Preconditions / replacement | Status | Evidence |
| --- | --- | --- | --- | --- |
| WP-1 | `inference/provider/router.rs` + tests | Crate `ModelRouter` owns live routing | PENDING | #4783 adopted the router; legacy provider consumers remain |
| WP-1 | `inference/provider/reliable.rs` + tests | Crate retry/fallback owns every model call | PENDING | Retargeted provider wire-parity tests |
| WP-1 | `inference/provider/legacy_provider.rs` and `compatible` alias | Every OpenAI-compatible slug uses crate `OpenAiModel` | PENDING | #4780/#4782/#4784 client cutover; residual legacy callers must be removed |
| WP-1 | `inference/provider/traits.rs` + tests | No `impl Provider`; consumers use crate model/message/usage types | PENDING | Consumer sweep + `inference_provider_e2e` and `agent_harness_e2e` |
| WP-1 | `tinyagents/model.rs::ProviderModel` / `MaxTokensModel` | Tier and bespoke models are direct `ChatModel`s | PENDING | `rg ProviderModel src` empty |
| WP-1 | `tinyagents/convert.rs` message conversion | No host `ChatMessage`; retain tool-schema conversion until WP-4 | PENDING | Conversion tests moved or retired |
| WP-1 | `inference/provider/crate_provider.rs` | No legacy `Provider` consumer needs the reverse adapter | PENDING | `rg 'impl Provider' src` empty |
| WP-2 | `routing/{policy,quality,factory}.rs` | Generic decisions use crate `ModelRouter`; host health signals remain | PENDING | Routing parity tests host/crate |
| WP-2 | `tool_timeout` implementation | Crate `ToolTimeout` owns timeout mechanics; host only projects config/env | PENDING | Timeout precedence tests |
| WP-2 | `model_council/{council,graph}.rs` | Generic ensemble graph released in tinyagents | PENDING | Upstream PR + offline graph tests |
| WP-3 | legacy `run_turn_engine` and graph escape hatches | All regression assertions exercise the crate turn path | PENDING | `rg OPENHUMAN_AGENT_GRAPH_` history-only |
| WP-4 | host tool trait/adapter artifacts selected by design | Approved tool-model decision preserves security and ungated result types | DESIGN GATE | Successor design document |
| WP-5 | generic seam middlewares | Equivalent crate middleware released and adopted | PENDING | Per-middleware drift rows + parity tests |
| WP-5 | detached subagent registry mechanics | Crate `TaskStore`/`SteeringRegistry` own lifecycle | PENDING | Upstream PR + orchestration tests |
| WP-5 | `agent/progress_tracing.rs` and `progress_tracing/langfuse.rs` | C4 S2-S6 gates pass; journal projection is self-sufficient | BLOCKED | One-release shadow parity and C4 §5 gate |

Deletion totals are reconciled in WP-6 after all rows are terminal. The
original projection is approximately 30k host LOC deleted and 12–15k generic
LOC upstreamed; measured totals, not the estimate, are authoritative.
