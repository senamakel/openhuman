//! Host layer over [`tinymemory_core::sync::composio`].
//!
//! The domain itself lives in the extracted crate; what stays here is its
//! JSON-RPC surface — handlers and controller schemas name OpenHuman's
//! `RpcOutcome` and `ControllerSchema`, which the engine crate cannot see.
//! The glob re-export keeps every historical `memory::sync::composio::…` path resolving.
//!
//! # This shim cannot go yet — but the reason changed, so read this before acting on it
//!
//! **The upstream blocker is closed.** This section used to say the loops could
//! not move because the pinned v1.5.0 module carried a heading reading *"The
//! periodic sync loops are deliberately NOT started here"* and three reasons
//! behind it: the pipeline read Composio credentials off `Config` rather than
//! the `ComposioHost` seam, `global::client_if_ready()` was `None` inside the
//! module, and `EngineRuntimeConfig::memory_sync_interval_secs()` answered
//! `Some(0)` (which the contract defines as *manual only*, so every tick would
//! skip every source silently).
//!
//! tinymemory#100 closed all three and shipped as **v1.6.0**, which is what
//! `modules::registry` and `modules::memory`'s `ARTIFACT_CAPABILITIES_PIN` now
//! pin. The host's half is wired too: `modules::ops` puts
//! `memory_sync_interval_secs`, `composio_mode` and `composio_entity_id` into
//! the `ModuleConfig` precisely so the module's own loops have a cadence and a
//! mode to run with. `core/runtime/services.rs` no longer starts either loop
//! and its comment block records why.
//!
//! # So what still pins this file, and it is not the loop
//!
//! The glob below is the only definition of every name in this list, and each
//! one has a live caller and no capability family:
//!
//! - `periodic` — reached by `integrations::composio::{mod, periodic}`, and by
//!   `super::bus`, which calls `periodic::record_sync_success` after a
//!   trigger-driven sync.
//! - `init_default_composio_sync_providers` + `all_composio_sync_providers` —
//!   `memory::sources::rpc`'s supported-toolkit list is built from exactly this
//!   pair; `get_composio_sync_provider` joins them in `tests/raw_coverage/`.
//! - `SyncTarget` / `list_sync_targets` — `memory::ops::sync`'s manual
//!   trigger path.
//! - `ComposioProvider`, `ProviderContext`, `ProviderUserProfile`,
//!   `SyncOutcome`, `SyncReason` — re-exported onward by
//!   `integrations::composio`.
//!
//! # What v1.7.0 took off that list, and what it did not
//!
//! **Running a connection is no longer on it.** `MemorySourceSync` landed in
//! tinymemory v1.7.0, so `run_connection_sync` / `source_sync_state` are
//! contract members and all five host call sites moved onto the bound driver:
//! `memory::ops::sync`, `super::bus`, `providers::slack::rpc` (both the trigger
//! and the status read) and `integrations::composio::ops::providers_ops`. None
//! of them names the engine crate any more. The member takes no `Config` —
//! the driver resolves its own and reads the per-source budgets from the
//! registry it already owns.
//!
//! **Two of the value types moved too, and the rest did not.** `SyncOutcome`,
//! `SyncReason`, `ProviderUserProfile`, `SyncState` and the scope/task
//! vocabulary are defined in `tinymemory-bus` now and re-exported here through
//! the engine, so a consumer that wants a *type* can repoint at
//! `tinymemory_api::composio::…` instead. What stayed behind is behaviour and
//! I/O: `ProviderContext` (an `Arc<Config>` plus host-seam dispatch), the
//! `ComposioProvider` trait and the provider registry, `profile_md`, and
//! `SyncState`'s `load` / `save` — the contract crate holds no traits and no
//! I/O, so those are the engine's `PersistedSyncState` extension trait. A host
//! site reaching for `load`/`save` is a site that should be calling
//! `source_sync_state` instead; the four above now do.
//!
//! So what still pins the glob is *target discovery and scheduling*:
//! `list_sync_targets` / `SyncTarget`, the provider registry, and `periodic`.
//! No capability family addresses those — `MemorySourceSink` covers source
//! *records*, not which connections exist — so this shim goes when the registry
//! moves behind the bus, which is a design pass upstream.
//!
//! # ⚠ One host caller did not get the memo (found 2026-08-25)
//!
//! `services.rs` dropped its `start_periodic_sync()` call, but
//! `channels/runtime/startup.rs` still makes one, through
//! `integrations::composio::start_periodic_sync` → `periodic` → this glob. That
//! is the *engine's* loop, started in this process, next to the module's own —
//! which is the case `services.rs` warns is "worse than a duplicate", because
//! the `cdylib` links its own `tinymemory-core` and neither loop's
//! `SCHEDULER_STARTED` `OnceLock` can see the other. Its store handle is a
//! second question: no production path calls `tinymemory_core::global::init`
//! any more — only test fixtures do — so whatever that loop reaches is not the
//! client the rest of the host uses.
//!
//! It is left alone here deliberately — that file is outside this change — but
//! do not read "the loops moved" as done until it goes. Composio sync degrades
//! **quietly** in either direction: a run that reports zero connections is
//! indistinguishable from a user who has none, so a half-removed loop looks
//! like a working one until someone notices their mail stopped being indexed.

pub use tinymemory_core::sync::composio::*;

pub mod providers;

pub mod bus;

pub use bus::{
    register_composio_trigger_subscriber, ComposioConfigChangedSubscriber,
    ComposioTriggerSubscriber,
};
