//! Phase guidance rendering, effective tool-scope resolution, and workflow
//! selection (auto-match over `when_to_use`).

use std::collections::HashSet;
use std::fmt::Write as _;

use super::types::{ToolScope, Workflow, WorkflowPhase};

/// Render a single phase's `rules` (and `scripts`, for transparency) as a
/// compact Markdown guidance block. Returns `None` when the phase has no
/// rules and no scripts (nothing worth injecting).
///
/// Script *output* is appended separately by the harness after it runs the
/// scripts; this block only describes the rules and the commands that will run.
pub fn phase_guidance(workflow: &Workflow, phase_name: &str) -> Option<String> {
    let phase = workflow.phase(phase_name)?;
    render_phase_guidance(&workflow.name, phase_name, phase)
}

pub(crate) fn render_phase_guidance(
    workflow_name: &str,
    phase_name: &str,
    phase: &WorkflowPhase,
) -> Option<String> {
    if phase.rules.is_empty() && phase.scripts.is_empty() {
        return None;
    }
    let mut out = String::new();
    let _ = writeln!(out, "## Workflow: {workflow_name} — phase `{phase_name}`");
    if let Some(desc) = phase.description.as_deref() {
        if !desc.trim().is_empty() {
            let _ = writeln!(out, "{}", desc.trim());
        }
    }
    if !phase.rules.is_empty() {
        out.push_str("\n**Rules for this phase:**\n");
        for rule in &phase.rules {
            let _ = writeln!(out, "- {}", rule.trim());
        }
    }
    if !phase.scripts.is_empty() {
        out.push_str("\n**Scripts run at this phase (output below):**\n");
        for script in &phase.scripts {
            let _ = writeln!(out, "- `{}`", script.trim());
        }
    }
    Some(out)
}

/// Resolve the effective [`ToolScope`] for a phase: the phase's own scope if it
/// declares one, otherwise the workflow-level default. Returns `None` when
/// neither is set (no tool narrowing for this phase).
pub fn effective_tool_scope(workflow: &Workflow, phase_name: &str) -> Option<ToolScope> {
    let phase_scope = workflow
        .phase(phase_name)
        .and_then(|p| p.tools.clone())
        .filter(|s| !s.is_empty());
    phase_scope.or_else(|| workflow.tools.clone().filter(|s| !s.is_empty()))
}

/// Apply a [`ToolScope`] to the agent's current visible-tool set, returning the
/// narrowed set. Scoping only ever narrows: `allow` (when non-empty) intersects
/// with `base`, and `deny` is removed afterwards. An empty `base` means "all
/// tools visible", so a non-empty `allow` becomes the new set.
pub fn narrow_visible_tools(base: &HashSet<String>, scope: &ToolScope) -> HashSet<String> {
    let mut result: HashSet<String> = if scope.allow.is_empty() {
        base.clone()
    } else if base.is_empty() {
        scope.allow.iter().cloned().collect()
    } else {
        scope
            .allow
            .iter()
            .filter(|t| base.contains(*t))
            .cloned()
            .collect()
    };
    for denied in &scope.deny {
        result.remove(denied);
    }
    result
}

/// Pick the best-matching workflow for a free-text task description, scoring by
/// keyword overlap between the task and each workflow's `when_to_use` (falling
/// back to `description`). Returns `None` when nothing scores above zero.
///
/// This is a deliberately simple lexical matcher — the agent itself does the
/// nuanced selection from the injected catalog; this helper backs the explicit
/// "find a workflow for this task" RPC / auto-attach path.
pub fn best_match<'a>(workflows: &'a [Workflow], task: &str) -> Option<&'a Workflow> {
    let task_tokens = tokenize(task);
    if task_tokens.is_empty() {
        return None;
    }
    let mut best: Option<(&Workflow, usize)> = None;
    for workflow in workflows {
        let hint = workflow
            .when_to_use
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(&workflow.description);
        let hint_tokens = tokenize(hint);
        let score = hint_tokens
            .iter()
            .filter(|t| task_tokens.contains(*t))
            .count();
        if score == 0 {
            continue;
        }
        match best {
            Some((_, best_score)) if best_score >= score => {}
            _ => best = Some((workflow, score)),
        }
    }
    best.map(|(w, _)| w)
}

fn tokenize(text: &str) -> HashSet<String> {
    text.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|t| t.len() > 2)
        .map(|t| t.to_ascii_lowercase())
        .collect()
}
