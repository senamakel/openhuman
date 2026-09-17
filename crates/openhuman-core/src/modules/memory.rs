//! The host half of `ai.tinyhumans.tinymemory.Memory`.
//!
//! [`ModuleMemoryProvider`] implements `MemoryProvider` by forwarding each method
//! to the loaded module. Because the wire surface mirrors the trait one method
//! for one method, there is no translation layer here — only the bus call, the
//! error mapping, and the two decisions below.
//!
//! # Construction is synchronous and does no I/O
//!
//! `memory::binding::build` is called from `CoreContext::memory_binding`, which
//! roughly four thousand pre-boot tests invoke with no tokio runtime at all. So
//! [`ModuleMemoryProvider::new`] cannot load the module, cannot dial the bus, and
//! cannot await anything. It stores its configuration and resolves on first use,
//! the same lazy-loading contract used by the module host.
//!
//! That has one consequence worth stating plainly, because it looks like a
//! shortcut and is not:
//!
//! ## `capabilities()` is answered statically
//!
//! `MemoryProvider::capabilities` is a **synchronous** method, and the module can
//! only answer it over the bus. It therefore cannot be asked here.
//!
//! It does not need to be. The TinyMemory module serves the complete shared API,
//! and that is a property of the artifact's *source*, fixed at the version the
//! registry pins, not something to discover at runtime. So this returns
//! [`Capabilities::all`], and [`ModuleMemoryProvider::verify`] cross-checks it
//! against the module's own answer on first use and logs loudly on disagreement.
//!
//! Guessing high would be the dangerous direction: the kernel filters its RPC
//! surface and agent-tool assembly from this set, so an overstated capability
//! registers methods that answer errors. Guessing exactly is safe; the
//! cross-check catches a future artifact that widens its scope.
//!
//! # Errors round-trip through the shared table
//!
//! `tinymemory_api::wire` maps a `MemoryError` to a `(name, message)` pair and
//! back, and **both ends use it**. Reimplementing the mapping here is what would
//! let a `PathEscape` arrive as an `Invalid`, silently reclassifying a sandbox
//! escape as a caller mistake.
//!
//! # Module layout
//!
//! Split by responsibility rather than by trait count:
//! - [`capabilities`] — the pinned artifact's advertised capability set.
//! - [`provider`] — [`ModuleMemoryProvider`] construction, proxy resolution,
//!   and the bus-error/macro plumbing the trait impls below build on.
//! - `documents_tree`, `entities_graph_diff`, `goals_tools_sources`,
//!   `sync_sessions_episodic`, `people_chunks_retrieval`, `ingest_answer`,
//!   `core_provider` — the `Memory*` trait forwarding, grouped by the memory
//!   subsystem each family belongs to.

#[cfg(test)]
#[path = "memory_tests.rs"]
mod tests;

mod capabilities;
mod core_provider;
mod documents_tree;
mod entities_graph_diff;
mod goals_tools_sources;
mod ingest_answer;
mod people_chunks_retrieval;
mod provider;
mod sync_sessions_episodic;

pub(crate) use capabilities::ARTIFACT_CAPABILITIES;
#[cfg(test)]
pub(crate) use capabilities::{capabilities_for, ARTIFACT_CAPABILITIES_PIN};
#[cfg(test)]
use provider::from_bus;
pub(crate) use provider::policy;
pub use provider::{install_host_callbacks, publish_cli_boot_policy, set_modules_policy};
pub use provider::{ModuleMemoryProvider, MODULE_ID};
#[cfg(test)]
use sync_sessions_episodic::INGEST_BUS_GRACE;
