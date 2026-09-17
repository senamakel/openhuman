//! Where one agent's files live.
//!
//! Everything an agent owns sits under the runtime's workspace, keyed by the
//! agent id, using the core's own per-profile conventions so the desktop's
//! profile tooling and the agent harness read the same paths:
//!
//! ```text
//! <root>/
//!   config.toml, auth-profiles.json, core.token      runtime-wide
//!   workspace/
//!     session_db/sessions.db                         runtime-wide run ledger
//!     personalities/<id>/{SOUL.md, MEMORY.md, skills/}   the agent's home
//!     session_raw/<ts>_<id>.jsonl                    its transcripts
//!     session_raw-<id>/, memory-<id>/                with dedicated_memory
//!   agents/<id>/action/                              its default action_dir
//! ```
//!
//! Under [`Workspace::Inherit`](crate::Workspace) the same per-profile paths
//! are used inside the operator's workspace, and the default `action_dir`
//! becomes `<action_dir>/profiles/<id>` — the desktop's own dedicated-
//! workspace convention — so a library agent never invents a directory the
//! operator's install does not already know about.

use std::path::{Path, PathBuf};

use openhuman_core::agent::profiles::{
    effective_memory_suffix, profile_action_workspace, profile_home, profile_skills_dir,
    session_raw_subdir_for_suffix, AgentProfile,
};

/// Resolved per-agent paths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentLayout {
    /// `<workspace>/personalities/<id>/`.
    pub home: PathBuf,
    /// `<workspace>/personalities/<id>/skills/`.
    pub skills: PathBuf,
    /// The transcript directory the harness writes for this agent.
    pub transcripts: PathBuf,
    /// The agent's read/write root.
    pub action_dir: PathBuf,
}

impl AgentLayout {
    /// Lay out `profile` under `workspace_dir`, with `action_dir` either the
    /// spec's explicit choice or the default for this runtime's workspace
    /// kind.
    pub(crate) fn resolve(
        workspace_dir: &Path,
        profile: &AgentProfile,
        action_dir: PathBuf,
    ) -> Self {
        let suffix = effective_memory_suffix(profile);
        Self {
            home: profile_home(workspace_dir, &profile.id),
            skills: profile_skills_dir(workspace_dir, &profile.id),
            transcripts: workspace_dir.join(session_raw_subdir_for_suffix(&suffix)),
            action_dir,
        }
    }

    /// The default `action_dir` for agent `id`.
    ///
    /// Runtime-owned roots get `<root>/agents/<id>/action` — a sibling of the
    /// workspace, never inside it, because `is_workspace_internal_path`
    /// blocks agent writes beneath the workspace fail-closed. An inherited
    /// workspace gets the desktop's `<action_dir>/profiles/<id>`.
    pub(crate) fn default_action_dir(
        root_dir: &Path,
        runtime_action_dir: &Path,
        inherited: bool,
        id: &str,
    ) -> PathBuf {
        if inherited {
            profile_action_workspace(runtime_action_dir, id)
        } else {
            root_dir.join("agents").join(id).join("action")
        }
    }
}

#[cfg(test)]
#[path = "layout_tests.rs"]
mod tests;
