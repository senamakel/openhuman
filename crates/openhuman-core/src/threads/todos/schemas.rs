//! JSON-RPC surface for the per-thread todo list. Pairs with the agent
//! `todo` tool — both call into [`super::ops`] so user-driven and
//! agent-driven edits share the exact same persistence and rendering
//! logic.

#[cfg(test)]
#[path = "schemas_tests.rs"]
mod tests;

mod handlers;
mod registry;
mod schema_defs;

pub use registry::{all_controller_schemas, all_registered_controllers};
