//! Public discovery entry points: the workspace-trust check and the handful
//! of `load_workflow_metadata*` / `discover_workflows*` shims that select a
//! root scan (see [`super::scan`]) for a given caller shape.

use std::path::Path;

use crate::skills::ops_types::{Workflow, TRUST_MARKER};

use super::scan::{discover_filtered, ALL_ROOT_KINDS, WORKFLOW_ROOT_KINDS};

/// Initialize the legacy skills directory in the specified workspace.
///
/// Creates `<workspace>/skills/` and a placeholder `README.md` so the folder
/// is visible to the user. New-style skills should live under
/// `<workspace>/.openhuman/skills/` instead, but this directory is kept for
/// backward compatibility.
pub fn init_workflows_dir(workspace_dir: &Path) -> Result<(), String> {
    let skills_dir = workspace_dir.join("skills");
    std::fs::create_dir_all(&skills_dir).map_err(|e| {
        format!(
            "failed to create skills directory {}: {e}",
            skills_dir.display()
        )
    })?;

    let readme_path = skills_dir.join("README.md");
    if !readme_path.exists() {
        let content = "# Skills\n\nPut one skill per directory under this folder.\n";
        std::fs::write(&readme_path, content)
            .map_err(|e| format!("failed to write {}: {e}", readme_path.display()))?;
    }

    Ok(())
}

/// Backwards-compatible shim for callers that only have a workspace path.
///
/// Delegates to [`discover_workflows`] with the current user's home directory
/// so user-scope skills (`~/.openhuman/skills/`, `~/.agents/skills/`) are
/// surfaced for existing production callers (`agent::harness::session::builder`,
/// `channels::runtime::startup`). Previously this shim passed `None` for the
/// home directory, which silently dropped user-installed skills from the
/// main runtime path.
///
/// Project-scope (workspace) skills still take precedence over user-scope
/// on name collisions.
pub fn load_workflow_metadata(workspace_dir: &Path) -> Vec<Workflow> {
    let trusted = is_workspace_trusted(workspace_dir);
    let home = dirs::home_dir();
    discover_workflows_inner(home.as_deref(), Some(workspace_dir), None, trusted)
}

/// Like [`load_workflow_metadata`], but additionally scans a profile-local
/// skills root (`<workspace>/personalities/<id>/skills/`) when one is supplied.
///
/// Callers pass the active profile's root (resolved via
/// `profiles::profile_skills_root`) so the returned catalog carries that
/// profile's private skills. `None` reproduces [`load_workflow_metadata`]
/// byte-for-byte, so the profile-less session and every other profile are
/// unaffected. Profile-local skills win same-name collisions against global
/// scopes (see [`crate::skills::ops_types::WorkflowScope::Profile`]).
pub fn load_workflow_metadata_for_profile(
    workspace_dir: &Path,
    profile_skills_root: Option<&Path>,
) -> Vec<Workflow> {
    let trusted = is_workspace_trusted(workspace_dir);
    let home = dirs::home_dir();
    discover_workflows_inner(
        home.as_deref(),
        Some(workspace_dir),
        profile_skills_root,
        trusted,
    )
}

/// Discover skills from every supported location.
///
/// * `home_dir` — user home (typically `dirs::home_dir()`), scanned for
///   `~/.openhuman/skills/` and `~/.agents/skills/`.
/// * `workspace_dir` — current workspace, scanned for project-scope paths.
/// * `trusted` — whether the caller has verified the project trust marker.
///   Project-scope skills are silently skipped when `false`.
///
/// On name collisions, project-scope wins over user-scope and a warning is
/// attached to the retained skill.
pub fn discover_workflows(
    home_dir: Option<&Path>,
    workspace_dir: Option<&Path>,
    trusted: bool,
) -> Vec<Workflow> {
    discover_workflows_inner(home_dir, workspace_dir, None, trusted)
}

/// Discover skills including a profile-local root, for a turn running under a
/// specific agent profile.
///
/// `profile_skills_root` is `<workspace>/personalities/<id>/skills/` (resolved
/// via `profiles::profile_skills_root`, which validates the id). It is scanned
/// unconditionally — no trust marker is required, since the directory is
/// core-managed under `workspace_dir` — and its bundles win same-name collisions
/// against every global scope for this profile. `None` is identical to
/// [`discover_workflows`], so other profiles and the default session never see
/// these skills.
pub fn discover_workflows_with_profile(
    home_dir: Option<&Path>,
    workspace_dir: Option<&Path>,
    profile_skills_root: Option<&Path>,
    trusted: bool,
) -> Vec<Workflow> {
    #[cfg(test)]
    DISCOVERY_CALLS.with(|c| c.set(c.get() + 1));
    discover_workflows_inner(home_dir, workspace_dir, profile_skills_root, trusted)
}

#[cfg(test)]
thread_local! {
    /// Test-only counter of full on-disk discovery passes made on this thread.
    /// Discovery re-reads and re-parses every skill bundle under every root, so
    /// a caller that runs it twice for one lookup pays the whole tree twice
    /// (#6166). Thread-local so parallel tests can't perturb each other's count.
    pub(crate) static DISCOVERY_CALLS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Whether the workspace has opted into loading project-scope skills.
///
/// Looks for `<workspace>/.openhuman/trust`. The marker file's contents are
/// ignored — presence is sufficient.
pub fn is_workspace_trusted(workspace_dir: &Path) -> bool {
    workspace_dir.join(".openhuman").join(TRUST_MARKER).exists()
}

pub(crate) fn discover_workflows_inner(
    home_dir: Option<&Path>,
    workspace_dir: Option<&Path>,
    profile_skills_root: Option<&Path>,
    trusted: bool,
) -> Vec<Workflow> {
    discover_filtered(
        home_dir,
        workspace_dir,
        profile_skills_root,
        trusted,
        ALL_ROOT_KINDS,
    )
}

/// Discover only automation bundles under the `workflows/` roots.
/// Capability skills are deliberately excluded; they remain available to the
/// agent harness and run/describe paths.
///
/// Note: bundles authored *before* the skills→workflows rename live under the
/// `skills/` roots and will therefore not appear in this automations-only view;
/// new automations created via "New workflow" land in `~/.openhuman/workflows/`.
pub fn discover_automations(
    home_dir: Option<&Path>,
    workspace_dir: Option<&Path>,
    trusted: bool,
) -> Vec<Workflow> {
    tracing::debug!(
        trusted,
        has_home = home_dir.is_some(),
        has_workspace = workspace_dir.is_some(),
        "[workflows] discover:automations:enter"
    );
    discover_filtered(home_dir, workspace_dir, None, trusted, WORKFLOW_ROOT_KINDS)
}
