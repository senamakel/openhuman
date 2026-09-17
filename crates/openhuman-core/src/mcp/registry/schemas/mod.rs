//! Controller schemas and handler dispatch for the MCP clients domain.
//!
//! Every `schemas(function)` match arm defines the RPC method's input/output
//! shape. Every `handle_*` function deserialises params and delegates to
//! `ops.rs`.

#[cfg(test)]
#[path = "../schemas_tests.rs"]
mod tests;

mod handlers;
mod params;
mod registry;
mod setup_handlers;
mod setup_registry;

pub use registry::{all_controller_schemas, all_registered_controllers, schemas};

// Test-only bridges: `schemas_tests.rs` (kept as-is; not part of the unsplit
// batch) reaches these through `use super::*`, mirroring the flat scope it
// had when `include!` spliced everything into one file.
#[cfg(test)]
use params::{read_optional_u32, read_required, type_name};
#[cfg(test)]
pub(crate) use setup_registry::setup_schemas;
