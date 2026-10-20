//! Native doctor checks for locally configured modules and services.

mod ids;
mod mcp;
mod modules;
mod registry;
mod sandbox;
mod skills;

pub(super) use ids::canonical_segment;
pub(super) use registry::for_context;
