//! Skill registry: browse, search, and install skills from remote registries.
//!
//! Supports multiple source formats (OpenHuman SKILL.md, Hermes, OpenClaw)
//! unified behind a single catalog interface with local caching.

pub mod ops;
pub mod schemas;
pub mod store;
pub mod types;

pub use schemas::{
    all_skill_registry_controller_schemas, all_skill_registry_registered_controllers,
};
