//! Workflow lifecycle operations: scaffold, read, and uninstall.

use serde::Deserialize;
use std::path::Path;

use super::discover::{discover_workflows_inner, is_workspace_trusted};
use super::types::{
    Workflow, WorkflowScope, MAX_DESCRIPTION_LEN, MAX_NAME_LEN, PHASE_CLOSE_TASK,
    PHASE_ENTER_DIRECTORY, PHASE_PICK_UP_TASK, WORKFLOW_MD,
};

/// Input for [`create_workflow`]. Mirrors the `workflows.create` payload.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct CreateWorkflowParams {
    /// Human-readable name — slugified into the on-disk folder.
    pub name: String,
    /// One-line description written into the frontmatter.
    pub description: String,
    /// Natural-language "when to use" hint (drives auto-match).
    #[serde(default)]
    pub when_to_use: Option<String>,
    /// Where to install: `user` (default) or `project`.
    #[serde(default)]
    pub scope: WorkflowScope,
    /// Optional tags.
    #[serde(default)]
    pub tags: Vec<String>,
}

/// Outcome of [`uninstall_workflow`].
#[derive(Debug, Clone)]
pub struct UninstallWorkflowOutcome {
    pub name: String,
    pub removed_path: String,
    pub scope: WorkflowScope,
}

/// Resolve a single workflow by its on-disk slug / id, running the standard
/// discovery pipeline so the read is scoped to legitimately installed
/// workflows.
pub fn read_workflow(workspace_dir: &Path, workflow_id: &str) -> Result<Workflow, String> {
    if workflow_id.trim().is_empty() {
        return Err("workflow_id must not be empty".to_string());
    }
    let home = dirs::home_dir();
    let trusted = is_workspace_trusted(workspace_dir);
    discover_workflows_inner(home.as_deref(), Some(workspace_dir), trusted)
        .into_iter()
        .find(|w| w.id() == workflow_id || w.name == workflow_id)
        .ok_or_else(|| format!("workflow '{workflow_id}' not found"))
}

/// Scaffold a new `WORKFLOW.md`-based workflow on disk.
///
/// Writes `<scope-root>/<slug>/WORKFLOW.md` with frontmatter derived from
/// `params` plus a starter set of the three canonical phases. Returns the
/// freshly-discovered workflow.
pub fn create_workflow(
    workspace_dir: &Path,
    params: CreateWorkflowParams,
) -> Result<Workflow, String> {
    let home = dirs::home_dir();
    create_workflow_inner(home.as_deref(), workspace_dir, params)
}

pub(crate) fn create_workflow_inner(
    home_dir: Option<&Path>,
    workspace_dir: &Path,
    params: CreateWorkflowParams,
) -> Result<Workflow, String> {
    tracing::debug!(
        name = %params.name,
        scope = ?params.scope,
        workspace = %workspace_dir.display(),
        "[workflows] create_workflow: entry"
    );

    let display_name = params.name.trim();
    if display_name.is_empty() {
        return Err("name must not be empty".to_string());
    }
    if display_name.len() > MAX_NAME_LEN {
        return Err(format!("name exceeds max {MAX_NAME_LEN} chars"));
    }

    let description = params.description.trim();
    if description.is_empty() {
        return Err("description must not be empty".to_string());
    }
    if description.len() > MAX_DESCRIPTION_LEN {
        return Err(format!(
            "description exceeds max {MAX_DESCRIPTION_LEN} chars"
        ));
    }

    let slug = slugify_workflow_name(display_name)?;

    let scope_root = scope_root(home_dir, workspace_dir, params.scope)?;
    std::fs::create_dir_all(&scope_root).map_err(|e| {
        format!(
            "failed to create workflows root {}: {e}",
            scope_root.display()
        )
    })?;

    let canonical_root = std::fs::canonicalize(&scope_root).map_err(|e| {
        format!(
            "failed to canonicalize workflows root {}: {e}",
            scope_root.display()
        )
    })?;

    let workflow_dir = canonical_root.join(&slug);
    if !workflow_dir.starts_with(&canonical_root) {
        return Err(format!(
            "resolved workflow dir {} escapes scope root {}",
            workflow_dir.display(),
            canonical_root.display(),
        ));
    }
    if workflow_dir.exists() {
        return Err(format!(
            "workflow '{slug}' already exists at {}",
            workflow_dir.display()
        ));
    }

    std::fs::create_dir_all(&workflow_dir).map_err(|e| {
        format!(
            "failed to create workflow dir {}: {e}",
            workflow_dir.display()
        )
    })?;

    let workflow_md_path = workflow_dir.join(WORKFLOW_MD);
    let rendered = render_workflow_md(
        &slug,
        description,
        params.when_to_use.as_deref(),
        &params.tags,
    );
    std::fs::write(&workflow_md_path, rendered)
        .map_err(|e| format!("failed to write {}: {e}", workflow_md_path.display()))?;

    tracing::info!(
        slug = %slug,
        scope = ?params.scope,
        location = %workflow_md_path.display(),
        "[workflows] create_workflow: wrote WORKFLOW.md"
    );

    let trusted = is_workspace_trusted(workspace_dir);
    discover_workflows_inner(home_dir, Some(workspace_dir), trusted)
        .into_iter()
        .find(|w| w.id() == slug)
        .ok_or_else(|| format!("created workflow '{slug}' but failed to re-discover"))
}

/// Input for [`uninstall_workflow`].
#[derive(Debug, Clone, Deserialize)]
pub struct UninstallWorkflowParams {
    /// Exact on-disk slug of the installed workflow.
    pub name: String,
}

/// Remove an installed **user-scope** workflow from
/// `~/.openhuman/workflows/<name>/`. Project-scope workflows are read-only.
/// Rejects path separators / traversal and canonicalises before delete.
pub fn uninstall_workflow(
    params: UninstallWorkflowParams,
    home_override: Option<&Path>,
) -> Result<UninstallWorkflowOutcome, String> {
    let name = params.name.trim();
    if name.is_empty() {
        return Err("name must not be empty".to_string());
    }
    if name.contains('/') || name.contains('\\') || name.contains("..") {
        return Err("name must be a plain slug (no path separators)".to_string());
    }

    let home = match home_override {
        Some(h) => h.to_path_buf(),
        None => {
            dirs::home_dir().ok_or_else(|| "could not resolve user home directory".to_string())?
        }
    };
    let root = home.join(".openhuman").join("workflows");
    let canonical_root = std::fs::canonicalize(&root)
        .map_err(|e| format!("workflows root {} not found: {e}", root.display()))?;

    let target = canonical_root.join(name);
    let canonical_target =
        std::fs::canonicalize(&target).map_err(|e| format!("workflow '{name}' not found: {e}"))?;
    if !canonical_target.starts_with(&canonical_root) {
        return Err(format!(
            "workflow path escapes workflows root: {}",
            canonical_target.display()
        ));
    }
    if !canonical_target.is_dir() {
        return Err(format!("workflow '{name}' is not a directory"));
    }

    std::fs::remove_dir_all(&canonical_target)
        .map_err(|e| format!("failed to remove {}: {e}", canonical_target.display()))?;

    tracing::info!(name = %name, path = %canonical_target.display(), "[workflows] uninstall_workflow: removed");

    Ok(UninstallWorkflowOutcome {
        name: name.to_string(),
        removed_path: canonical_target.display().to_string(),
        scope: WorkflowScope::User,
    })
}

fn scope_root(
    home_dir: Option<&Path>,
    workspace_dir: &Path,
    scope: WorkflowScope,
) -> Result<std::path::PathBuf, String> {
    match scope {
        WorkflowScope::User => {
            let home =
                home_dir.ok_or_else(|| "could not resolve user home directory".to_string())?;
            Ok(home.join(".openhuman").join("workflows"))
        }
        WorkflowScope::Project => {
            if !is_workspace_trusted(workspace_dir) {
                return Err(format!(
                    "workspace {} is not trusted; create {}/.openhuman/trust to enable project-scope workflows",
                    workspace_dir.display(),
                    workspace_dir.display(),
                ));
            }
            Ok(workspace_dir.join(".openhuman").join("workflows"))
        }
    }
}

/// Convert a human-readable workflow name to a filesystem-safe slug.
/// Same rules as `skills::slugify_skill_name`.
pub(crate) fn slugify_workflow_name(name: &str) -> Result<String, String> {
    let mut out = String::new();
    let mut prev_hyphen = true;
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            prev_hyphen = false;
        } else if (ch == '-' || ch == '_' || ch.is_whitespace()) && !prev_hyphen {
            out.push('-');
            prev_hyphen = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        return Err(format!(
            "name '{name}' has no alphanumeric characters; cannot derive slug"
        ));
    }
    if out.len() > MAX_NAME_LEN {
        return Err(format!("slug '{out}' exceeds max {MAX_NAME_LEN} chars"));
    }
    Ok(out)
}

/// Render a starter `WORKFLOW.md` with the three canonical phases stubbed out.
pub(crate) fn render_workflow_md(
    slug: &str,
    description: &str,
    when_to_use: Option<&str>,
    tags: &[String],
) -> String {
    let mut out = String::new();
    out.push_str("---\n");
    out.push_str(&format!("name: {slug}\n"));
    out.push_str(&format!("description: {}\n", yaml_scalar(description)));
    if let Some(w) = when_to_use {
        out.push_str(&format!("when_to_use: {}\n", yaml_scalar(w)));
    }
    if !tags.is_empty() {
        out.push_str("tags:\n");
        for t in tags {
            out.push_str(&format!("  - {}\n", yaml_scalar(t)));
        }
    }
    out.push_str("phases:\n");
    out.push_str(&format!("  {PHASE_ENTER_DIRECTORY}:\n"));
    out.push_str("    rules:\n");
    out.push_str("      - Review the local README before editing.\n");
    out.push_str("    context: [git]\n");
    out.push_str(&format!("  {PHASE_PICK_UP_TASK}:\n"));
    out.push_str("    rules:\n");
    out.push_str("      - State your plan before making changes.\n");
    out.push_str(&format!("  {PHASE_CLOSE_TASK}:\n"));
    out.push_str("    rules:\n");
    out.push_str("      - Verify the work before closing the task.\n");
    out.push_str("---\n\n");
    out.push_str(&format!("# {slug}\n\n"));
    out.push_str(description);
    if !description.ends_with('\n') {
        out.push('\n');
    }
    out.push_str("\n_Describe the workflow's phases, rules, and scripts above._\n");
    out
}

/// Best-effort YAML scalar encoder (copied from `skills::ops_create`).
pub(crate) fn yaml_scalar(s: &str) -> String {
    let needs_quote = s.is_empty()
        || s.chars().any(|c| {
            matches!(
                c,
                ':' | '#'
                    | '\''
                    | '"'
                    | '\n'
                    | '\r'
                    | '\t'
                    | '['
                    | ']'
                    | '{'
                    | '}'
                    | ','
                    | '&'
                    | '*'
                    | '!'
                    | '|'
                    | '>'
                    | '%'
                    | '@'
                    | '`'
            )
        })
        || s.starts_with(|c: char| c.is_ascii_whitespace() || c == '-' || c == '?')
        || s.ends_with(|c: char| c.is_ascii_whitespace());
    if !needs_quote {
        return s.to_string();
    }
    let escaped = s
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t");
    format!("\"{escaped}\"")
}
