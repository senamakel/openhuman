//! Loadable native modules: capabilities that live outside this binary.
//!
//! A module is a compiled `cdylib` that speaks the tinybus module ABI. It is
//! downloaded from a pinned release, verified against a digest compiled into
//! [`registry`], admitted through tinybus's ABI and manifest gates, and attached
//! to a private in-process broker as an ordinary bus peer. The core then calls it
//! over that bus like any other service.
//!
//! # What this buys, and what it costs
//!
//! It buys a dependency boundary that survives compilation. A document writer, a
//! codec, an integration SDK — none of these are kernel work, and each one drags
//! a tree of parsers into a binary that mostly does something else. Moving one
//! out removes its dependencies from the build rather than merely gating them.
//!
//! It costs process isolation, and that is not a small thing. A loaded module
//! shares this address space, these privileges and this crash domain: tinybus's
//! deadlines, bounded queues and caught panics contain ordinary misbehaviour, not
//! a segfault. `dlopen` also runs code before any symbol can be inspected, so the
//! ABI, manifest and digest gates decide what is *admitted*, never what is
//! *safe*. Modules are first-party code that happens to ship separately. Anything
//! untrusted belongs in a process, not here.
//!
//! And tinybus never unloads a library. A module that fails is failed until the
//! process restarts, which is why [`ops`] caches failures rather than retrying.
//!
//! # Layout
//!
//! - [`registry`] — the compiled-in set of loadable modules and their digests.
//! - `documents` (with the `documents` feature) — the host half of the `tinydocs`
//!   module's three operations.
//! - [`browser`] and [`browser_task`] — TinyComputer browser members and tasks
//!   without linking the browser engine into the core; `site_memory_host` serves
//!   where the module keeps what finished tasks learned about each site, and
//!   `profile_claim` asks the module to claim the persistent Chrome profile for
//!   a browser about to start.
//! - `wallet` (with the `web3` feature) — the host half of the `tinywallet`
//!   module, which builds and assembles transactions while the signing key
//!   stays in this process.
//! - [`linked`] — the registry of modules linked into the executable (local development).
//! - [`runtime`] — the process-wide `tinybus::module::ModuleRuntime` and the host
//!   callbacks served on its bus.
//! - [`ops`] — the adapter over `tinybus::module::Resolver` and `ModuleClient`:
//!   builds them from [`Config`](crate::config::Config) and the registry.
//! - `report` — the Sentry sink for terminal module outcomes.
//! - [`client`] — the constructor for a `tinybus::module::ModuleClient`, with or without the loader.
//! - [`schemas`] — the `modules` RPC surface.
//! - [`boot`] — what happens at startup.

pub mod accessibility;
#[cfg(feature = "modules")]
pub mod boot;
#[cfg(feature = "channels")]
pub(crate) mod channel_controller;
pub(crate) mod channel_health;
#[cfg(all(feature = "channels", feature = "modules"))]
pub(crate) mod channel_runtime;
#[cfg(feature = "modules")]
pub mod computer;
#[cfg(feature = "modules")]
pub mod computer_config;
#[cfg(feature = "modules")]
pub mod connectors;
#[cfg(feature = "modules")]
pub mod desktop;
#[cfg(feature = "documents")]
#[cfg(feature = "modules")]
pub mod documents;
mod envelope;
#[cfg(feature = "modules")]
pub mod linked;
#[cfg(feature = "modules")]
pub mod ops;
#[cfg(feature = "modules")]
pub(crate) mod profile_claim;
pub mod registry;
pub(crate) mod report;
#[cfg(feature = "modules")]
pub mod runtime;
#[cfg(feature = "modules")]
pub mod schemas;
#[cfg(feature = "modules")]
pub mod search;
#[cfg(feature = "modules")]
pub(crate) mod site_memory_host;
mod source;
#[cfg(feature = "modules")]
mod storage_grant_tinybox;
#[cfg(feature = "modules")]
mod storage_grant_tinychannels;
#[cfg(feature = "modules")]
mod storage_grant_tinyconnectors;
#[cfg(feature = "modules")]
mod storage_grant_tinyjuice;
#[cfg(feature = "modules")]
mod storage_grant_tinymcp;
#[cfg(feature = "modules")]
mod storage_grant_tinywallet;
#[cfg(feature = "modules")]
pub mod storage_grants;
#[cfg(feature = "modules")]
pub(crate) mod storage_host;
#[cfg(feature = "modules")]
mod storage_host_linked;
#[cfg(all(test, feature = "modules"))]
mod storage_host_watching;
#[cfg(feature = "modules")]
pub mod tinybox;
pub(crate) mod tinyflows;
#[cfg(feature = "modules")]
mod tokenjuice_host;
pub mod types;
#[cfg(not(feature = "modules"))]
mod unloaded;
#[cfg(feature = "voice")]
#[cfg(feature = "modules")]
pub mod voice;
#[cfg(feature = "web3")]
#[cfg(feature = "modules")]
pub mod wallet;
#[cfg(feature = "web3")]
mod wallet_host;
#[cfg(feature = "web3")]
pub(crate) mod wallet_service;
#[cfg(feature = "web3")]
pub(crate) mod web3_service;
#[cfg(feature = "web3")]
pub(crate) mod x402_service;

#[cfg(feature = "modules")]
pub use ops::{ensure_loaded, ensure_loaded_within, state_of};
#[cfg(feature = "modules")]
pub use schemas::{all_controller_schemas, all_registered_controllers};
#[doc(hidden)]
pub use source::local_artifact;
/// A `tinybus::module::ModuleClient` that loads modules lazily through the
/// process-wide resolver (see [`ops`]). Constructing one starts nothing. In a
/// build without the `modules` feature there is no loader, and every call is
/// unavailable and reported once with the `loader_disabled` reason.
#[must_use]
pub fn client(config: crate::config::Config) -> tinybus::module::ModuleClient {
    #[cfg(feature = "modules")]
    {
        ops::client(config)
    }
    #[cfg(not(feature = "modules"))]
    {
        unloaded::client(config)
    }
}

pub use types::{LoadPolicy, ModuleRecord, ModuleSource, ModuleState, ModuleStatus, PlatformAsset};
