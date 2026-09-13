//! Workflow discovery: scanning root directories, scope resolution, collision handling,
//! and skill resource reading.
//!
//! Split into submodules by responsibility: [`api`] holds the public
//! discovery entry points, [`scan`] holds the root-directory scan engine,
//! [`collision`] holds cross-scope name-collision resolution, and
//! [`resource`] holds bundled-resource reading.

mod api;
mod collision;
mod resource;
mod scan;

// Re-imported here (rather than only inside the submodules that need them)
// so `use super::*` in the `#[path]`-included test files below still finds
// `Path`, `Workflow`, `WorkflowScope`, and `precedence` exactly as it did
// before this module was split into submodules.
#[cfg(test)]
use super::ops_types::{Workflow, WorkflowScope};
#[cfg(test)]
use collision::precedence;
#[cfg(test)]
use std::path::Path;

pub use api::{
    discover_automations, discover_workflows, discover_workflows_with_profile, init_workflows_dir,
    is_workspace_trusted, load_workflow_metadata, load_workflow_metadata_for_profile,
};
pub use resource::{
    profile_local_skill_ids, read_workflow_resource, read_workflow_resource_with_profile,
};

pub(crate) use api::discover_workflows_inner;

#[cfg(test)]
pub(crate) use api::DISCOVERY_CALLS;

#[cfg(test)]
#[path = "ops_discover_include_skills_tests_tests.rs"]
mod include_skills_tests;
#[cfg(test)]
#[path = "ops_discover_profile_scope_tests_tests.rs"]
mod profile_scope_tests;
