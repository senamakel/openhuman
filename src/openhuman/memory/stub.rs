//! The `memory`-disabled surface.
//!
//! # What an off-build must still be able to say
//!
//! Roughly 230 production references across ~101 files outside this family
//! reach into it, and they do not agree on what "off" should mean. Two shapes
//! are needed, and picking one for everything would be wrong:
//!
//! - **Registration sites want absence.** The controller registry in
//!   `core/all.rs` calls the `all_*_registered_controllers` aggregators
//!   unconditionally. A stub that returned a controller answering
//!   `Err("memory disabled")` would make `memory.*` a *known* method that
//!   fails at run time — the opposite of the intended unknown-method. So the
//!   aggregators return empty vectors, and `core/all.rs` needs no `#[cfg]` at
//!   all.
//! - **Hot-path readers want a no-op.** The agent harness recalls memory on
//!   every turn and the archivist records every turn. Those callers are always
//!   compiled and do not branch on a feature, so their entry points here
//!   return "nothing" rather than an error. That is not a new behaviour: it is
//!   exactly what `NullMemoryProvider` already produces at run time when no
//!   module is available, and what `DomainSet` models with `memory: false`.
//!
//! Anything that *writes* returns a build-fact error naming the gate, because
//! silently discarding a write is the one failure mode that looks like success.
//!
//! # The rule for editing this file
//!
//! Signatures must match the real ones exactly. The disabled build is the
//! **only** thing that catches drift here — CI's smoke lane runs `cargo check`,
//! not `cargo test --no-default-features` — so run
//!
//! ```text
//! cargo check --no-default-features --features "media,skills,flows,mcp,channels,medulla,http-server,scheduler-gate,file-logging,modules"
//! ```
//!
//! before pushing any change to the memory surface.
//!
//! Do **not** add types here. Inert types belong in `memory::api` (the contract
//! re-export) or `memory::ranking`, both of which stay compiled in every build
//! precisely so there is one definition rather than two.

use crate::core::all::RegisteredController;
use crate::core::types::ControllerSchema;

/// The message every disabled write path returns.
///
/// A build fact, phrased as one: a caller reading this in a log should know to
/// rebuild rather than to check their configuration.
pub const MEMORY_DISABLED: &str =
    "memory feature disabled at compile time — rebuild with `--features memory`";

macro_rules! empty_controller_aggregators {
    ($($schemas:ident / $controllers:ident),* $(,)?) => {
        $(
            /// No schemas: the family is compiled out, so its methods are
            /// unknown rather than known-and-failing.
            #[must_use]
            pub fn $schemas() -> Vec<ControllerSchema> {
                Vec::new()
            }

            /// No controllers, for the same reason as the schema half.
            #[must_use]
            pub fn $controllers() -> Vec<RegisteredController> {
                Vec::new()
            }
        )*
    };
}

empty_controller_aggregators!(
    all_memory_controller_schemas / all_memory_registered_controllers,
    all_memory_core_recall_controller_schemas / all_memory_core_recall_registered_controllers,
    all_memory_documents_controller_schemas / all_memory_documents_registered_controllers,
    all_memory_files_controller_schemas / all_memory_files_registered_controllers,
    all_memory_ingest_controller_schemas / all_memory_ingest_registered_controllers,
    all_memory_kv_graph_controller_schemas / all_memory_kv_graph_registered_controllers,
    all_memory_learn_controller_schemas / all_memory_learn_registered_controllers,
    all_memory_provider_controller_schemas / all_memory_provider_registered_controllers,
    all_memory_sync_controller_schemas / all_memory_sync_registered_controllers,
    all_memory_tool_memory_controller_schemas / all_memory_tool_memory_registered_controllers,
);
