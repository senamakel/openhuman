//! Host layer over [`tinymemory_core::sync::composio::providers`].
//!
//! The domain itself lives in the extracted crate; what stays here is its
//! JSON-RPC surface — handlers and controller schemas name OpenHuman's
//! `RpcOutcome` and `ControllerSchema`, which the engine crate cannot see.
//! The glob re-export keeps every historical `memory::sync::composio::providers::…` path resolving.
//!
//! # The widest shim in the memory tree, and the least ambiguous (#5560)
//!
//! `integrations::composio::providers` is `pub use …providers::*` over this
//! module, so the whole engine registry is public surface under two names: the
//! six per-toolkit providers (`clickup`, `github`, `gmail`, `linear`, `notion`,
//! `slack`), the curated catalogs, `tool_scope`, `user_scopes`, `sync_state`,
//! `profile` / `profile_md`, and the `ComposioProvider` trait itself. Over a
//! hundred call sites across `src/` and `tests/` reach it under one of the two
//! — `flows`, the agent capability matrix, `memory::sources`, `read_rpc`, the
//! composio RPC ops.
//!
//! There is nothing to route it through. The provider registry *is* the sync
//! pipeline, no capability family addresses it, and the contract's
//! `MemorySourceSink` covers source **records** rather than provider runs. So
//! unlike the tree shims, this one is not blocked on a type identity or on a
//! release — it is blocked on the sync pipelines moving behind the bus, which
//! is a design pass upstream. Note the local `pub mod slack` below shadows the
//! engine's `slack` from this glob; see that file before assuming either is
//! droppable.

pub use tinymemory_core::sync::composio::providers::*;

pub mod slack;
