//! Workflow discovery: scanning root directories, scope resolution, and
//! collision handling. Mirrors `skills::ops_discover`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::parse::load_from_workflow_md;
use super::types::{Workflow, WorkflowScope, WORKFLOW_MD};

/// Whether the workspace has opted into loading project-scope workflows.
///
/// Reuses the same `<workspace>/.openhuman/trust` marker the skills subsystem
/// uses, so trusting a workspace enables project-scope skills *and* workflows.
pub fn is_workspace_trusted(workspace_dir: &Path) -> bool {
    crate::openhuman::skills::is_workspace_trusted(workspace_dir)
}

/// Discover workflows from every supported location.
///
/// * `home_dir` — user home (typically `dirs::home_dir()`), scanned for
///   `~/.openhuman/workflows/`.
/// * `workspace_dir` — current workspace, scanned for project-scope paths.
/// * `trusted` — whether the caller has verified the project trust marker.
///   Project-scope workflows are silently skipped when `false`.
///
/// On name collisions, project-scope wins over user-scope and a warning is
/// attached to the retained workflow.
pub fn discover_workflows(
    home_dir: Option<&Path>,
    workspace_dir: Option<&Path>,
    trusted: bool,
) -> Vec<Workflow> {
    discover_workflows_inner(home_dir, workspace_dir, trusted)
}

/// Backwards-compatible shim for callers that only have a workspace path.
/// Resolves the user home itself so user-scope workflows are surfaced.
pub fn load_workflows(workspace_dir: &Path) -> Vec<Workflow> {
    let trusted = is_workspace_trusted(workspace_dir);
    let home = dirs::home_dir();
    discover_workflows_inner(home.as_deref(), Some(workspace_dir), trusted)
}

pub(crate) fn discover_workflows_inner(
    home_dir: Option<&Path>,
    workspace_dir: Option<&Path>,
    trusted: bool,
) -> Vec<Workflow> {
    // Scan user first, then project, so project-scope registers last and wins
    // on a name collision.
    let mut by_name: HashMap<String, Workflow> = HashMap::new();

    if let Some(home) = home_dir {
        for root in user_roots(home) {
            absorb(&mut by_name, scan_root(&root, WorkflowScope::User));
        }
    }

    if let Some(ws) = workspace_dir {
        if trusted {
            for root in project_roots(ws) {
                absorb(&mut by_name, scan_root(&root, WorkflowScope::Project));
            }
        }
    }

    let mut out: Vec<Workflow> = by_name.into_values().collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

fn user_roots(home: &Path) -> Vec<PathBuf> {
    vec![home.join(".openhuman").join("workflows")]
}

fn project_roots(workspace: &Path) -> Vec<PathBuf> {
    vec![workspace.join(".openhuman").join("workflows")]
}

fn absorb(by_name: &mut HashMap<String, Workflow>, incoming: Vec<Workflow>) {
    for mut workflow in incoming {
        let key = workflow.name.clone();
        if let Some(existing) = by_name.remove(&key) {
            let (winner, loser) = if precedence(workflow.scope) >= precedence(existing.scope) {
                (&mut workflow, existing)
            } else {
                let mut kept = existing;
                kept.warnings.push(format!(
                    "name '{}' also declared in {:?} scope at {} (ignored)",
                    kept.name,
                    workflow.scope,
                    workflow
                        .location
                        .as_deref()
                        .map(|p| p.display().to_string())
                        .unwrap_or_else(|| "<unknown>".to_string())
                ));
                by_name.insert(key, kept);
                continue;
            };
            winner.warnings.push(format!(
                "shadowed {:?}-scope workflow at {} with same name",
                loser.scope,
                loser
                    .location
                    .as_deref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "<unknown>".to_string())
            ));
        }
        by_name.insert(key, workflow);
    }
}

fn precedence(scope: WorkflowScope) -> u8 {
    match scope {
        WorkflowScope::User => 1,
        WorkflowScope::Project => 2,
    }
}

fn scan_root(root: &Path, scope: WorkflowScope) -> Vec<Workflow> {
    let entries = match std::fs::read_dir(root) {
        Ok(entries) => entries,
        Err(_) => return Vec::new(),
    };

    // `read_dir` order is unspecified; sort by on-disk directory name for a
    // stable, reproducible collision-resolution order.
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|entry| entry.file_name());

    let mut out = Vec::new();
    for entry in entries {
        // Use `file_type()` (not `is_dir()`) so a symlinked child cannot be
        // loaded as a workflow — `is_dir()` dereferences symlinks.
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_symlink() || !file_type.is_dir() {
            continue;
        }
        let path = entry.path();
        let dir_name = entry.file_name().to_string_lossy().to_string();
        if dir_name.starts_with('.') {
            continue;
        }
        if let Some(workflow) = load_workflow_dir(&path, &dir_name, scope) {
            out.push(workflow);
        }
    }
    out
}

fn load_workflow_dir(dir: &Path, dir_name: &str, scope: WorkflowScope) -> Option<Workflow> {
    let workflow_md = dir.join(WORKFLOW_MD);
    if workflow_md.exists() {
        return Some(load_from_workflow_md(&workflow_md, dir, dir_name, scope));
    }
    None
}
