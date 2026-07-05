# TinyAgents Drift Ledger (Phase 0)

**Purpose.** The TinyAgents migration spans `inference`, `tools`, and
`agent_orchestration`, while the OpenHuman host will keep evolving. This ledger
records the baseline used for the port plan and tracks which host-side drift must
be upstreamed, retained, or deleted before each phase cuts over.

- **DRIFT -> tinyagents PR** - generic engine behavior absent from the crate; port
  upstream before deleting the host copy.
- **HOST-OWNED** - OpenHuman product policy, RPC, config, credentials, UI, local
  runtime, or integration glue. No upstream action.
- **CONSOLIDATE / DELETE** - duplicate host implementation already covered by
  TinyAgents primitives; delete only after the seam proves the crate-backed path.
- **CLOSED** - resolved by a submodule/version bump or a completed cutover.

> **Gate rule:** no phase deletes host code until every open row for that phase
> is either upstreamed and bumped, reclassified host-owned, or covered by a
> crate-backed seam test.

## Anchors

| Thing | Value |
| --- | --- |
| Host repo | `tinyhumansai/openhuman` |
| Host branch | `docs/tinyagents-port-plan` |
| Host audit base | `42ce5c0e9` (`origin/main`, 2026-07-04) |
| Plan commit | `24f200e49` (`docs: TinyAgents port plan`) |
| TinyAgents submodule | `vendor/tinyagents` -> `tinyhumansai/tinyagents` |
| Phase 0 target | `v1.6.0` / `e72036d847b589044aa9a4add1b34544b92a293d` |

## Baseline Snapshot

Recorded from `docs/tinyagents-port-plan` after the Phase 0 version alignment
work started. Counts include Rust files only.

| Host module | Rust files | LOC | Test fns | Plan disposition |
| --- | ---: | ---: | ---: | --- |
| `src/openhuman/inference/` | 116 | 53,023 | 1,101 | Provider consolidation and small generic ports |
| `src/openhuman/tools/` | 94 | 38,553 | 877 | Tool model reconciliation, then builtin family ports |
| `src/openhuman/agent_orchestration/` | 64 | 25,769 | 262 | Sub-agent lifecycle consolidation onto TinyAgents graph/orchestration |
| `src/openhuman/tinyagents/` | 25 | 15,219 | 101 | Host seam; shrinks but remains OpenHuman-owned |

## Phase 0 Drift Rows

| # | Area | Status | Evidence / action |
| --- | --- | --- | --- |
| P0-1 | Version skew: host required `tinyagents = 1.5.0` while the intended engine baseline is `v1.6.0` | **CLOSED** | Root `Cargo.toml` now requires `1.6.0`; root and Tauri lockfiles resolve `tinyagents` `1.6.0`; `vendor/tinyagents` points at `e72036d` (`v1.6.0`). |
| P0-2 | `ToolCompleted` outcome was reconstructed through OpenHuman's `ToolFailureMap` side channel | **CLOSED** | `src/openhuman/tinyagents/observability.rs` consumes TinyAgents 1.6 `duration_ms`, `output_bytes`, and `error`; `ToolFailureMap` now only preserves OpenHuman's richer classified failure and legacy fallback fields. |
| P0-3 | TinyAgents 1.6 event constructor shape changed for local observability tests | **CLOSED** | Local constructors in `src/openhuman/tinyagents/observability.rs` include `ModelCompleted.started_at_ms` and the expanded `ToolCompleted` fields. |
| P0-4 | `invoke_stream` adoption in `src/openhuman/tinyagents/mod.rs` | **UPSTREAM PR OPEN** | `OpenHumanTinyAgentModel::invoke` still calls `invoke_streaming_in_context` when progress streaming is enabled. TinyAgents 1.6 `invoke_stream` yields caller-consumable `AgentStreamItem`s but constructs a fresh `RunContext`, while OpenHuman needs cancellation, stores, workspace, events, steering, and journaling attached before drive. TinyAgents draft PR [tinyagents#21](https://github.com/tinyhumansai/tinyagents/pull/21) adds `invoke_stream_in_context`; host cutover waits for that merge/release and a later submodule bump. |
| P0-5 | SHA-256 prompt fingerprint / prompt-cache drift guard | **CLOSED** | `src/openhuman/tinyagents/middleware.rs` now stamps `PromptCacheSegmentMiddleware` segment ids and `ModelRequest::prompt_fingerprint` with SHA-256 over canonical JSON. Tool-cache identity includes the full serialized `ToolSchema` list, not just tool names, matching TinyAgents 1.6 `PromptBuilder::fingerprint` expectations. Added `prompt_cache_segments_fingerprint_full_tool_schema` as the local regression guard. |
| P0-6 | Idempotent redaction middleware vs `journal.rs` double-redaction | **CLOSED** | Audit found no OpenHuman install of TinyAgents `RedactionMiddleware`. Model-facing tool output is scrubbed once by `CredentialScrubMiddleware`; durable event persistence is separately wrapped by `journal.rs` `RedactingSink` over `openhuman_redaction_secrets()`. These protect different surfaces, so there is no crate/host double-redaction seam to collapse in Phase 0. |

## Phase 1 Drift Rows

| # | Area | Status | Evidence / action |
| --- | --- | --- | --- |
| P1-1 | `SchemaCleanr` provider schema normalization | **UPSTREAM PR OPEN** | TinyAgents draft PR [tinyagents#20](https://github.com/tinyhumansai/tinyagents/pull/20) ports `SchemaCleanr` into `harness::tool` with crate-native errors and ported tests. Host submodule remains pinned to `v1.6.0` until the upstream PR merges/releases; no host call-site deletion yet. Local validation in the submodule: `cargo fmt --check`; `timeout 180s cargo clippy --all-targets -- -D warnings`; `timeout 120s cargo test schema_`. |
| P1-2 | `current_time` / `resolve_time` builtin tool pilot | **UPSTREAM PR OPEN** | TinyAgents draft PR [tinyagents#22](https://github.com/tinyhumansai/tinyagents/pull/22) adds an optional `tools` feature and `harness::tools::{CurrentTimeTool, ResolveTimeTool, time_tools, register_time_tools}`. `chrono` / `chrono-tz` stay feature-gated and out of the default dependency set. Host wrappers remain in place until the upstream PR merges/releases and the host bumps the submodule. Local validation in the submodule: `cargo fmt --check`; `timeout 240s cargo clippy --features tools --all-targets -- -D warnings`; `timeout 180s cargo test --features tools time_`. |

## Phase Gates

| Phase | Gate rows | Status |
| --- | --- | --- |
| Phase 0 - version alignment | P0-1, P0-2, P0-3, P0-4, P0-5, P0-6 | **UPSTREAM PR OPEN** |
| Phase 1 - quick upstream ports | SchemaCleanr, error classification, model context, reasoning channel, worktree isolation, display metadata, time tools | **IN PROGRESS** |
| Phase 2 - tool model and builtin families | ToolResult structure, permission model, ToolAccess, edit tracking, filesystem/network/time tools | **NOT STARTED** |
| Phase 3 - provider consolidation | OpenAI-compatible provider cutover, retry ownership, backend envelope split | **NOT STARTED** |
| Phase 4 - orchestration consolidation | TaskStore/SteeringRegistry lifecycle, status vocabulary, session durability | **NOT STARTED** |
| Phase 5 - workflow/team generic slices | Validation/scheduling slice evaluation | **NOT STARTED** |
| Phase 6 - cleanup and docs | Transitional shim deletion and architecture docs | **NOT STARTED** |

## Closing Procedure

1. For a **DRIFT -> tinyagents PR** row, branch inside `vendor/tinyagents`, port
   the generic change with crate-native tests, merge/release upstream, then bump
   the host submodule and version pin together.
2. For a **HOST-OWNED** row, document the boundary and keep the logic in
   OpenHuman behind the seam.
3. For a **CONSOLIDATE / DELETE** row, add or update the seam proof first, cut the
   live path to TinyAgents, then delete the duplicate host implementation.
4. Update this ledger in the same host PR that closes or reclassifies a row.
