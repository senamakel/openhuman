//! Core types, constants, and phase-name helpers for the agent_workflows
//! subsystem.
//!
//! A **workflow** is a directory containing a `WORKFLOW.md` file with YAML
//! frontmatter. Unlike a skill (a single guidance document), a workflow binds
//! behaviour to **lifecycle phases of a task** (e.g. picking up a task, closing
//! a task, entering a directory). At each phase the harness can inject the
//! phase's `rules` as guidance, run its `scripts` through the gated shell, scope
//! the agent's visible `tools`, and surface working-directory `context`.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

pub(crate) const WORKFLOW_MD: &str = "WORKFLOW.md";
pub(crate) const MAX_NAME_LEN: usize = 64;
pub(crate) const MAX_DESCRIPTION_LEN: usize = 1024;

/// Canonical v1 phase name: a task is started / moved to in-progress.
pub const PHASE_PICK_UP_TASK: &str = "on_pick_up_task";
/// Canonical v1 phase name: a task is completed / closed.
pub const PHASE_CLOSE_TASK: &str = "on_close_task";
/// Canonical v1 phase name: the agent changes into / starts working in a dir.
pub const PHASE_ENTER_DIRECTORY: &str = "on_enter_directory";

/// The phase names recognised + documented in v1. Workflows may declare other
/// (custom) phase names; these are simply the ones the harness auto-detects.
pub const CANONICAL_PHASES: &[&str] =
    &[PHASE_PICK_UP_TASK, PHASE_CLOSE_TASK, PHASE_ENTER_DIRECTORY];

/// Working-directory context provider keys understood by `workdir.rs`.
pub const CONTEXT_GIT: &str = "git";

/// Where the workflow was discovered. Determines precedence on name collision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WorkflowScope {
    /// Workflow shipped with the user's global config
    /// (`~/.openhuman/workflows/...`).
    User,
    /// Workflow shipped with the current workspace
    /// (`<ws>/.openhuman/workflows/...`). Requires the trust marker to load.
    Project,
}

impl Default for WorkflowScope {
    fn default() -> Self {
        Self::User
    }
}

/// Tool-visibility scoping for a workflow or one of its phases.
///
/// Both lists name tools by their registered `Tool::name()`. `allow`, when
/// non-empty, narrows the agent's visible tools to (its existing allowlist ∩
/// `allow`); `deny` removes tools regardless. Scoping only ever *narrows* — it
/// never grants a tool the agent was not already permitted.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolScope {
    #[serde(default)]
    pub allow: Vec<String>,
    #[serde(default)]
    pub deny: Vec<String>,
}

impl ToolScope {
    pub fn is_empty(&self) -> bool {
        self.allow.is_empty() && self.deny.is_empty()
    }
}

/// One lifecycle phase of a workflow.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkflowPhase {
    /// Optional human-readable note about the phase.
    #[serde(default)]
    pub description: Option<String>,
    /// Guidance rules injected into the agent's context at this phase.
    #[serde(default)]
    pub rules: Vec<String>,
    /// Shell scripts run (through the gated `ShellTool`) at phase entry; their
    /// output is injected back into context.
    #[serde(default)]
    pub scripts: Vec<String>,
    /// Per-phase tool-visibility override. When `None`, the workflow-level
    /// default scope (if any) applies.
    #[serde(default)]
    pub tools: Option<ToolScope>,
    /// Working-directory property providers to surface at this phase
    /// (e.g. `["git"]`).
    #[serde(default)]
    pub context: Vec<String>,
}

/// Parsed YAML frontmatter of a `WORKFLOW.md` file.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkflowFrontmatter {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Natural-language hint describing when this workflow applies; drives both
    /// auto-match and the catalog the agent picks from.
    #[serde(default)]
    pub when_to_use: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    /// Workflow-level default tool scope; individual phases may override.
    #[serde(default)]
    pub tools: Option<ToolScope>,
    /// Phase name → phase definition. `BTreeMap` for stable ordering.
    #[serde(default)]
    pub phases: BTreeMap<String, WorkflowPhase>,
    /// Forward-compat hatch for unrecognised top-level keys.
    #[serde(flatten)]
    pub extra: HashMap<String, serde_yaml::Value>,
}

/// A discovered workflow.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Workflow {
    /// Display name (from frontmatter, falls back to directory name).
    pub name: String,
    /// On-disk slug — the directory under the scope root. This is the
    /// identifier the read / uninstall RPCs resolve against.
    #[serde(default)]
    pub dir_name: String,
    /// Short description used in the catalog summary.
    pub description: String,
    /// When this workflow applies (auto-match + catalog hint).
    #[serde(default)]
    pub when_to_use: Option<String>,
    /// Tags declared in frontmatter.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Workflow-level default tool scope.
    #[serde(default)]
    pub tools: Option<ToolScope>,
    /// Phase name → phase definition.
    #[serde(default)]
    pub phases: BTreeMap<String, WorkflowPhase>,
    /// Path to the `WORKFLOW.md` file.
    pub location: Option<PathBuf>,
    /// Where the workflow came from.
    #[serde(default)]
    pub scope: WorkflowScope,
    /// Non-fatal parse warnings, surfaced in the catalog for user debugging.
    #[serde(default)]
    pub warnings: Vec<String>,
}

impl Workflow {
    /// The identifier callers resolve against (the on-disk slug, falling back to
    /// the display name for back-compat with values written before `dir_name`
    /// existed).
    pub fn id(&self) -> &str {
        if self.dir_name.is_empty() {
            &self.name
        } else {
            &self.dir_name
        }
    }

    /// Look up a phase by name.
    pub fn phase(&self, name: &str) -> Option<&WorkflowPhase> {
        self.phases.get(name)
    }
}
