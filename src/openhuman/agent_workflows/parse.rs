//! `WORKFLOW.md` parsing: splitting frontmatter from body and building a
//! [`Workflow`] from a parsed document.

use std::path::Path;

use super::types::{
    Workflow, WorkflowFrontmatter, WorkflowScope, MAX_DESCRIPTION_LEN, MAX_NAME_LEN,
};

/// Split a `WORKFLOW.md` file into parsed frontmatter, the remaining body, and
/// any parse-level diagnostics.
///
/// Accepts frontmatter delimited by leading `---` lines. Returns `None` when
/// the file cannot be read or the frontmatter block is opened with `---` but
/// never terminated. Mirrors `skills::parse_skill_md`.
pub fn parse_workflow_md(path: &Path) -> Option<(WorkflowFrontmatter, String, Vec<String>)> {
    let content = std::fs::read_to_string(path).ok()?;
    parse_workflow_md_str(&content)
}

/// Content-only variant of [`parse_workflow_md`].
pub fn parse_workflow_md_str(content: &str) -> Option<(WorkflowFrontmatter, String, Vec<String>)> {
    let mut lines = content.lines();
    let first = lines.next()?;
    if first.trim() != "---" {
        // No frontmatter — treat whole file as body.
        return Some((
            WorkflowFrontmatter::default(),
            content.to_string(),
            Vec::new(),
        ));
    }

    let mut yaml = String::new();
    let mut terminated = false;
    let mut body = String::new();
    for line in lines {
        if line.trim() == "---" {
            terminated = true;
            continue;
        }
        if !terminated {
            yaml.push_str(line);
            yaml.push('\n');
        } else {
            body.push_str(line);
            body.push('\n');
        }
    }

    if !terminated {
        return None;
    }

    let mut parse_warnings = Vec::new();
    let frontmatter = match serde_yaml::from_str::<WorkflowFrontmatter>(&yaml) {
        Ok(fm) => fm,
        Err(err) => {
            log::warn!("[workflows] failed to parse frontmatter: {err}");
            parse_warnings.push(format!("frontmatter parse error: {err}"));
            WorkflowFrontmatter::default()
        }
    };

    Some((frontmatter, body, parse_warnings))
}

pub(crate) fn first_body_line(body: &str) -> Option<String> {
    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        return Some(trimmed.to_string());
    }
    None
}

/// Load a [`Workflow`] from a `WORKFLOW.md` file. Collects non-fatal warnings
/// (missing/mismatched name, oversized fields, no phases) rather than failing,
/// so the catalog can surface the real cause.
pub(crate) fn load_from_workflow_md(
    workflow_md: &Path,
    dir: &Path,
    dir_name: &str,
    scope: WorkflowScope,
) -> Workflow {
    let _ = dir; // reserved for future bundled-resource inventory
    let mut warnings = Vec::new();
    let (frontmatter, body) = match parse_workflow_md(workflow_md) {
        Some((fm, body, parse_warnings)) => {
            warnings.extend(parse_warnings);
            (fm, body)
        }
        None => {
            warnings.push(format!(
                "could not parse {} — exposing directory as placeholder",
                workflow_md.display()
            ));
            (WorkflowFrontmatter::default(), String::new())
        }
    };

    let name = if frontmatter.name.trim().is_empty() {
        warnings.push("frontmatter missing 'name'; using directory name".to_string());
        dir_name.to_string()
    } else {
        if frontmatter.name != dir_name {
            warnings.push(format!(
                "frontmatter name '{}' does not match directory '{}'",
                frontmatter.name, dir_name
            ));
        }
        if frontmatter.name.len() > MAX_NAME_LEN {
            warnings.push(format!(
                "frontmatter name is {} chars (max recommended: {})",
                frontmatter.name.len(),
                MAX_NAME_LEN
            ));
        }
        frontmatter.name.clone()
    };

    let description = if frontmatter.description.trim().is_empty() {
        warnings
            .push("frontmatter missing 'description'; falling back to first body line".to_string());
        first_body_line(&body).unwrap_or_else(|| "No description provided".to_string())
    } else {
        if frontmatter.description.len() > MAX_DESCRIPTION_LEN {
            warnings.push(format!(
                "description is {} chars (max recommended: {})",
                frontmatter.description.len(),
                MAX_DESCRIPTION_LEN
            ));
        }
        frontmatter.description.clone()
    };

    if frontmatter.phases.is_empty() {
        warnings
            .push("workflow declares no phases; it will surface nothing at runtime".to_string());
    }

    Workflow {
        name,
        dir_name: dir_name.to_string(),
        description,
        when_to_use: frontmatter.when_to_use.clone(),
        tags: frontmatter.tags.clone(),
        tools: frontmatter.tools.clone(),
        phases: frontmatter.phases.clone(),
        location: Some(workflow_md.to_path_buf()),
        scope,
        warnings,
    }
}
