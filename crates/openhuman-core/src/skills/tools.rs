//! LLM-callable wrappers over the `workflows` metadata domain.
//!
//! These tools let the agent discover installed workflows, inspect a
//! workflow's definition and bundled resources, review recent runs and their
//! logs, and (opt-in) scaffold / install / uninstall user workflows. Thin
//! shims over the free functions in the `workflows::ops_*` / `registry` /
//! `run_log` modules.
//!
//! NOTE: launching a workflow is exposed separately by `RunWorkflowTool`
//! (`run_workflow`) + `AwaitWorkflowTool`, so it is not duplicated here.
//!
//! Read tools are default-enabled. The write/install/uninstall tools
//! (`create_workflow`, `install_workflow_from_url`, `uninstall_workflow`)
//! mutate the on-disk workflow set (and install fetches remote content), so
//! they ship default-OFF via `tools/user_filter.rs`.
//!
//! Split into submodules by responsibility: [`helpers`] holds the shared
//! argument-parsing and allowlist helpers, [`read`] holds the read-only
//! tools, and [`write`] holds the mutating ones.

#[cfg(test)]
use std::sync::Arc;

#[cfg(test)]
use serde_json::json;

#[cfg(test)]
use crate::config::Config;
#[cfg(test)]
use crate::tools::traits::{PermissionLevel, Tool};

mod helpers;
mod read;
mod write;

pub(super) use helpers::{skill_allowed, SkillAllowlist};
pub use read::{
    WorkflowDescribeTool, WorkflowListTool, WorkflowReadResourceTool, WorkflowReadRunLogTool,
    WorkflowRecentRunsTool,
};
pub use write::{WorkflowCreateTool, WorkflowInstallFromUrlTool, WorkflowUninstallTool};

#[cfg(test)]
#[path = "tools_tests.rs"]
mod tests;
