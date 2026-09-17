---
description: >-
  OpenHuman's host layer over the Memory Tree engine - the JSON-RPC handlers,
  controller schemas, CLI and event subscriber that stayed in the core after
  the tree mechanics moved to the vendored TinyMemory module.
icon: diagram-project
---

# Memory Tree (`crates/openhuman-core/src/memory/tree/`)

> **Status.** The generic tree mechanics (append, cascade seal, summarise,
> score, embed, retrieve) are crate-owned: they live in `vendor/tinymemory`
> (`tinymemory-core`, with `tinycortex` vendored beneath it) and are reached
> through the `tinymemory-api` contract. `crates/openhuman-core/src/memory/tree/`
> is OpenHuman's **host layer** over that engine: RPC handlers and controller
> schemas that name OpenHuman's `RpcOutcome` and `ControllerSchema`, which the
> engine crate cannot see. Nothing here opens SQLite or branches on tree kind.

The user-facing feature is described in [Memory Tree](../../features/obsidian-wiki/memory-tree.md).

```text
memory (orchestrator, crates/openhuman-core/src/memory/)
   │  binding.provider() → tinymemory-api contract
   ▼
memory/tree/              (this directory — host surface only)
   ├── tree/              memory_tree write/status RPCs + canonical ingest payloads
   ├── retrieval/         memory_tree read RPCs (query_source, drill_down, …)
   ├── tree_runtime/      tree_summarizer RPCs, `tree-summarizer` CLI, bus subscriber
   └── health/            pipeline failure taxonomy + doctor report
   │
   ▼
vendor/tinymemory         (engine: tinymemory-core / tinycortex — persistence, seal, score)
```

## Layout

| Path             | Role                                                                                                                                                                                                                                                                                                   |
| ---------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `mod.rs`         | Declares the four submodules and re-exports the three controller registries wired into `core/all.rs`. Its module doc records why the old `pub use tinymemory_core::tree::*` glob was removed (#5560).                                                                                                  |
| `tree/`          | Host wrappers mirroring `tinymemory_core::tree::tree`: the `memory_tree` write/status handlers in `rpc.rs` (`ingest_rpc`, `list_chunks_rpc`, `get_chunk_rpc`, `backfill_status_rpc`, `pipeline_status_rpc`, `doctor_rpc`, `retry_failed_rpc`, `set_enabled_rpc`) and the chat/email/document ingest payload shapes in `canonicalize_types.rs`. |
| `retrieval/`     | Agent-facing read RPCs (`rpc.rs`, `schemas.rs`): `query_source`, `cover_window`, `search_entities`, `drill_down`, `fetch_leaves`. `rpc.rs` calls the contract on the **unguarded** `binding.provider()` and passes `source_scope::as_bus_scope()` explicitly on every scoped call.                    |
| `tree_runtime/`  | The markdown time tree's host surface: `ops.rs` / `schemas.rs` (JSON-RPC), `cli.rs` (`tree-summarizer` CLI), `bus.rs` (event subscriber). Reaches the engine through the contract's runtime doors (`runtime_buffer_write`, `runtime_read_node`, `runtime_read_children`, `runtime_tree_status`, `runtime_summarize`, `runtime_rebuild`). |
| `health/`        | Pipeline failure taxonomy (`FailureCode`, `FailureClass`, `PipelineFailure`, `DegradedState`), the `user_error` wire payload, and the doctor `report` read from the bound driver's `MemoryMaintenance`. Distinct from `tinymemory_api::health::MemoryHealth` (driver liveness).                      |

## Layer rules

- **No tree mechanics here.** Seal cascades, summarisation, scoring, embedding and entity extraction happen inside the engine. This directory converts RPC requests into contract calls and contract results into `RpcOutcome`.
- **No persistence here.** The engine owns the tree tables; the host never opens them directly.
- **Scope is explicit.** Task-local state does not cross the module boundary, so every scoped call passes its source scope and self-echo exclusions as arguments.

## Controller registries

`mod.rs` re-exports three registries that `crates/openhuman-core/src/core/all.rs` wires into the global registry:

- `all_memory_tree_registered_controllers` (sourced from `memory/schema/`): the core `memory_tree` namespace — `ingest`, `list_chunks`, `get_chunk`, `pipeline_status`, `set_enabled`, `doctor`, `retry_failed`, `memory_backfill_status`, `smart_walk`, plus the `memory/read_rpc/` methods.
- `all_retrieval_registered_controllers` (`retrieval/schemas.rs`): also under `memory_tree` — `query_source`, `cover_window`, `search_entities`, `drill_down`, `fetch_leaves`.
- `all_tree_summarizer_registered_controllers` (`tree_runtime/schemas.rs`): the `tree_summarizer` namespace — `ingest`, `run`, `query`, `status`, `rebuild`.

RPC namespace strings are wire contracts; they did not change when the directory moved from `memory_tree/` to `memory/tree/`.

## Related

- [`crates/openhuman-core/src/memory/tree/README.md`](https://github.com/tinyhumansai/openhuman/blob/main/crates/openhuman-core/src/memory/tree/README.md): the internal-audience overview this page mirrors.
- [`crates/openhuman-core/src/memory/README.md`](https://github.com/tinyhumansai/openhuman/blob/main/crates/openhuman-core/src/memory/README.md): the memory domain and the engine extraction.
- `memory/query/`: the agent-facing `memory_tree` tool (`MemoryQueryTool`), which reuses `retrieval/rpc.rs` DTOs but goes through the guarded driver.
- [Memory Tree feature](../../features/obsidian-wiki/memory-tree.md): what end users see.
- [Architecture overview](../architecture.md): where this fits in the wider system.
