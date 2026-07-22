# TinyAgents migration deletion ledger

This ledger records host-side removals required by
[`tinyagents-migration-plan-2026-07-22.md`](../tinyagents-migration-plan-2026-07-22.md).
A row moves to `DELETED` only after its crate-backed replacement and the named
parity evidence are in place. Generic code moved into `vendor/tinyagents` must
also name its upstream PR before the host copy is removed.

| Work package | Host artifact | Preconditions / replacement | Status | Evidence |
| --- | --- | --- | --- | --- |
| WP-1 | `inference/provider/router.rs` + tests | Crate `ModelRouter` owns live routing | DELETED | `fcd3f3331`; #4783 adopted the router; root `cargo check` green |
| WP-1 | `inference/provider/reliable.rs` + tests | Crate retry/fallback owns every model call | DELETED | `fcd3f3331`; crate retry/fallback plus root `cargo check` green |
| WP-1 | Legacy compatible raw-coverage trio | Wire parity retained against crate `OpenAiModel` | DELETED | `4750defb0`; all 14 `inference_provider_e2e` tests green |
| WP-1 | `inference/provider/legacy_provider.rs` and `compatible` alias | Every OpenAI-compatible slug uses crate `OpenAiModel` | DELETED | #4780/#4782/#4784 plus native wire/SSE parity tests; root check and raw-coverage target compile green |
| WP-1 | `inference/provider/traits.rs` + tests | No `impl Provider`; consumers use crate model/message/usage types | PENDING | Consumer sweep + `inference_provider_e2e` and `agent_harness_e2e` |
| WP-1 | `tinyagents/model.rs::ProviderModel` / `MaxTokensModel` | Tier and bespoke models are direct `ChatModel`s | PENDING | `rg ProviderModel src` empty |
| WP-1 | `tinyagents/convert.rs` message conversion | No host `ChatMessage`; retain tool-schema conversion until WP-4 | PENDING | Conversion tests moved or retired |
| WP-1 | `inference/provider/crate_provider.rs` | No legacy `Provider` consumer needs the reverse adapter | PENDING | `rg 'impl Provider' src` empty |
| WP-1 | `inference/provider/{auth_error_registry,resolved_route,temperature,thread_context}.rs` | Host state/policy lives outside the legacy provider abstraction | REHOMED | `4ce6ca726`, `59871c8ab`, `6cb17f91e`, `07f675ba3`; root `cargo check` and focused auth-registry tests green |
| WP-2 | `routing/` parallel implementation | Generic decisions use crate `ModelRouter`; no live consumer remained | DELETED | Repository-wide reference audit found the module self-contained; #4783 owns live routing; root check green |
| WP-2 | `tool_timeout` implementation | No deletion: crate `ToolTimeout` is per-tool metadata, while the host owns global config/env state and enforces the adapter deadline | HOST-OWNED | Execution-path audit; timeout precedence/deadline tests |
| WP-2 | `model_council/graph.rs` | Crate `parallel::map_reduce` owns generic ordered fan-out | ALREADY ADOPTED | Existing map/reduce implementation + offline fan-out tests |
| WP-2 | `model_council/council.rs` | Product juror construction, model selection, prompt/result semantics, and limits | HOST-OWNED | Execution-path audit; council unit tests |
| WP-2 | `tool_status/` | OpenHuman security markers, serialized UI/persistence taxonomy, retry categories, and remediation copy | HOST-OWNED | Consumer audit; classifier/type unit tests |
| WP-3 | legacy `run_turn_engine` and graph escape hatches | All regression assertions exercise the crate turn path | ALREADY DELETED | Audit found no engine definition or runtime env read; session and subagent paths call TinyAgents unconditionally. Removed the stale runner comment. |
| WP-4 | host tool trait/adapter artifacts selected by design | Approved tool-model decision preserves security and ungated result types | DESIGN GATE | Successor design document |
| WP-5 | generic seam middlewares | Equivalent crate middleware released and adopted | PENDING | Per-middleware drift rows + parity tests |
| WP-5 | detached subagent registry mechanics | Crate `TaskStore`/`SteeringRegistry` own lifecycle | PENDING | Upstream PR + orchestration tests |
| WP-5 | `agent/progress_tracing.rs` and `progress_tracing/langfuse.rs` | C4 S2-S6 gates pass; journal projection is self-sufficient | BLOCKED | One-release shadow parity and C4 §5 gate |

Deletion totals are reconciled in WP-6 after all rows are terminal. The
original projection is approximately 30k host LOC deleted and 12–15k generic
LOC upstreamed; measured totals, not the estimate, are authoritative.
