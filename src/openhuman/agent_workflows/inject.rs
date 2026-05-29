//! Prompt-side rendering: the workflow **catalog** the agent sees so it can
//! pick a workflow matching the current task, then pull its phases via the
//! `workflow_load` tool.

use std::fmt::Write as _;

use super::types::Workflow;

/// Render the available-workflows catalog as an XML-ish block, analogous to
/// `render_available_skills()` in the integrations agent prompt. Returns an
/// empty string when there are no workflows so callers can append
/// unconditionally.
pub fn render_workflow_catalog(workflows: &[Workflow]) -> String {
    if workflows.is_empty() {
        return String::new();
    }
    let mut out = String::from("## Available Workflows\n\n<available_workflows>\n");
    for workflow in workflows {
        let phases: Vec<&str> = workflow.phases.keys().map(|k| k.as_str()).collect();
        let _ = writeln!(
            out,
            "  <workflow>\n    <id>{}</id>\n    <name>{}</name>\n    <description>{}</description>\n    <when_to_use>{}</when_to_use>\n    <phases>{}</phases>\n  </workflow>",
            xml_escape(workflow.id()),
            xml_escape(&workflow.name),
            xml_escape(&workflow.description),
            xml_escape(workflow.when_to_use.as_deref().unwrap_or("")),
            xml_escape(&phases.join(", ")),
        );
    }
    out.push_str("</available_workflows>\n\n");
    out.push_str(
        "When the active task matches a workflow's `when_to_use`, call `workflow_load` with its \
         id to activate it; its phase rules, scripts, and tool scoping then apply automatically as \
         the task progresses.",
    );
    out
}

/// Minimal XML text escaper for catalog values.
pub(crate) fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
