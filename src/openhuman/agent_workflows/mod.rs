//! Agent workflows: phase-keyed bundles of rules, scripts, tool-scoping, and
//! working-directory context that steer how the agent approaches a task.
//!
//! A **workflow** is a directory containing a `WORKFLOW.md` file with YAML
//! frontmatter. It binds behaviour to lifecycle **phases** of a task
//! (`on_pick_up_task`, `on_close_task`, `on_enter_directory`, …). At each phase
//! the harness can:
//! - inject the phase's `rules` as guidance,
//! - run its `scripts` through the gated shell (output injected back),
//! - narrow the agent's visible `tools`, and
//! - surface working-directory `context` (e.g. git branch/status).
//!
//! There is no bespoke executor/DAG — phase scripts reuse the gated `ShellTool`
//! and everything else is context injected into the agent's turn.
//!
//! ## Module layout
//!
//! | Module | Contents |
//! |---|---|
//! | [`types`] | Core types, constants, phase-name helpers |
//! | [`parse`] | WORKFLOW.md frontmatter/body parsing |
//! | [`discover`] | Scanning root directories, scope resolution, collisions |
//! | [`ops`] | Scaffold / read / uninstall |
//! | [`select`] | Phase guidance, tool-scope resolution, auto-match |
//! | [`workdir`] | Working-directory context providers (git, …) |
//! | [`inject`] | Prompt-side catalog rendering |
//! | [`schemas`] | JSON-RPC / CLI controller surface |

pub mod discover;
pub mod inject;
pub mod ops;
pub mod parse;
pub mod schemas;
pub mod select;
pub mod types;
pub mod workdir;

pub use discover::{discover_workflows, is_workspace_trusted, load_workflows};
pub use inject::render_workflow_catalog;
pub use ops::{
    create_workflow, read_workflow, uninstall_workflow, CreateWorkflowParams,
    UninstallWorkflowOutcome, UninstallWorkflowParams,
};
pub use parse::{parse_workflow_md, parse_workflow_md_str};
pub use select::{best_match, effective_tool_scope, narrow_visible_tools, phase_guidance};
pub use types::{
    ToolScope, Workflow, WorkflowFrontmatter, WorkflowPhase, WorkflowScope, CANONICAL_PHASES,
    CONTEXT_GIT, PHASE_CLOSE_TASK, PHASE_ENTER_DIRECTORY, PHASE_PICK_UP_TASK,
};
pub use workdir::working_dir_context;

pub use schemas::{
    all_controller_schemas as all_agent_workflows_controller_schemas,
    all_registered_controllers as all_agent_workflows_registered_controllers,
    schemas as workflows_schemas,
};

#[cfg(test)]
pub(crate) use discover::discover_workflows_inner;
#[cfg(test)]
pub(crate) use ops::{create_workflow_inner, slugify_workflow_name};

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
