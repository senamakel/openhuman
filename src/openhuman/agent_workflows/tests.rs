//! Unit tests for the agent_workflows domain: parse, discover, select, ops.

use super::*;
use std::collections::HashSet;
use std::fs;
use std::path::Path;

const SAMPLE: &str = r#"---
name: bug-triage
description: How to handle an incoming bug report
when_to_use: a user reports a bug or something is broken
tags: [support, debugging]
tools:
  allow: [shell, git_operations]
phases:
  on_enter_directory:
    rules:
      - Read the local README before editing.
    context: [git]
  on_pick_up_task:
    rules:
      - Always reproduce before proposing a fix.
    scripts:
      - git fetch
    tools:
      allow: [shell, git_operations, file_read]
  on_close_task:
    rules:
      - Never close without a passing test.
    scripts:
      - run tests
---
# Bug triage workflow

Overview prose here.
"#;

fn write_workflow(root: &Path, slug: &str, body: &str) {
    let dir = root.join(slug);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join(types::WORKFLOW_MD), body).unwrap();
}

// ── parse ────────────────────────────────────────────────────────────────

#[test]
fn parse_extracts_frontmatter_phases_and_body() {
    let (fm, body, warnings) = parse_workflow_md_str(SAMPLE).expect("parses");
    assert_eq!(fm.name, "bug-triage");
    assert_eq!(
        fm.when_to_use.as_deref(),
        Some("a user reports a bug or something is broken")
    );
    assert_eq!(fm.tags, vec!["support", "debugging"]);
    assert_eq!(fm.phases.len(), 3);
    let pick = fm.phases.get(PHASE_PICK_UP_TASK).unwrap();
    assert_eq!(pick.rules.len(), 1);
    assert_eq!(pick.scripts, vec!["git fetch"]);
    assert_eq!(
        pick.tools.as_ref().unwrap().allow,
        vec!["shell", "git_operations", "file_read"]
    );
    assert!(body.contains("Overview prose here."));
    assert!(warnings.is_empty());
}

#[test]
fn parse_no_frontmatter_treats_all_as_body() {
    let (fm, body, warnings) = parse_workflow_md_str("just a body\n").expect("parses");
    assert!(fm.name.is_empty());
    assert_eq!(body, "just a body\n");
    assert!(warnings.is_empty());
}

#[test]
fn parse_unterminated_frontmatter_returns_none() {
    assert!(parse_workflow_md_str("---\nname: x\nno terminator\n").is_none());
}

#[test]
fn parse_malformed_yaml_yields_warning_not_panic() {
    let (_fm, _body, warnings) =
        parse_workflow_md_str("---\nname: [unclosed\n---\nbody\n").expect("parses");
    assert!(warnings
        .iter()
        .any(|w| w.contains("frontmatter parse error")));
}

// ── discover ───────────────────────────────────────────────────────────────

#[test]
fn discover_finds_user_scope_workflow() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path();
    write_workflow(
        &home.join(".openhuman").join("workflows"),
        "bug-triage",
        SAMPLE,
    );

    let found = discover_workflows_inner(Some(home), None, false);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].name, "bug-triage");
    assert_eq!(found[0].scope, WorkflowScope::User);
    assert_eq!(found[0].phases.len(), 3);
}

#[test]
fn discover_project_scope_requires_trust() {
    let tmp = tempfile::tempdir().unwrap();
    let ws = tmp.path();
    write_workflow(
        &ws.join(".openhuman").join("workflows"),
        "proj-flow",
        SAMPLE,
    );

    // Untrusted: skipped.
    assert!(discover_workflows_inner(None, Some(ws), false).is_empty());
    // Trusted: surfaced.
    let found = discover_workflows_inner(None, Some(ws), true);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].scope, WorkflowScope::Project);
}

#[test]
fn discover_project_shadows_user_on_name_collision() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let ws = tmp.path().join("ws");
    write_workflow(&home.join(".openhuman").join("workflows"), "dup", SAMPLE);
    write_workflow(&ws.join(".openhuman").join("workflows"), "dup", SAMPLE);

    let found = discover_workflows_inner(Some(&home), Some(&ws), true);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].scope, WorkflowScope::Project);
    assert!(found[0].warnings.iter().any(|w| w.contains("shadowed")));
}

// ── ops: create / read / uninstall ─────────────────────────────────────────

#[test]
fn slugify_rejects_non_alphanumeric_and_lowercases() {
    assert_eq!(slugify_workflow_name("Bug Triage!").unwrap(), "bug-triage");
    assert!(slugify_workflow_name("!!!").is_err());
}

#[test]
fn create_then_read_round_trips() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    fs::create_dir_all(&home).unwrap();
    let ws = tmp.path().join("ws");
    fs::create_dir_all(&ws).unwrap();

    let created = create_workflow_inner(
        Some(&home),
        &ws,
        ops::CreateWorkflowParams {
            name: "My Flow".to_string(),
            description: "Does a thing".to_string(),
            when_to_use: Some("when X happens".to_string()),
            scope: WorkflowScope::User,
            tags: vec!["a".to_string()],
        },
    )
    .expect("creates");
    assert_eq!(created.id(), "my-flow");
    // Starter scaffold includes the three canonical phases.
    assert!(created.phases.contains_key(PHASE_PICK_UP_TASK));
    assert!(created.phases.contains_key(PHASE_CLOSE_TASK));
    assert!(created.phases.contains_key(PHASE_ENTER_DIRECTORY));

    // Re-create is rejected (never overwrites).
    assert!(create_workflow_inner(
        Some(&home),
        &ws,
        ops::CreateWorkflowParams {
            name: "My Flow".to_string(),
            description: "dup".to_string(),
            when_to_use: None,
            scope: WorkflowScope::User,
            tags: vec![],
        },
    )
    .is_err());
}

#[test]
fn uninstall_rejects_path_separators() {
    let err = uninstall_workflow(
        ops::UninstallWorkflowParams {
            name: "../evil".to_string(),
        },
        None,
    )
    .unwrap_err();
    assert!(err.contains("plain slug"));
}

// ── select: phase guidance, tool scope, match ──────────────────────────────

fn sample_workflow() -> Workflow {
    let (fm, _body, _w) = parse_workflow_md_str(SAMPLE).unwrap();
    Workflow {
        name: fm.name,
        dir_name: "bug-triage".to_string(),
        description: fm.description,
        when_to_use: fm.when_to_use,
        tags: fm.tags,
        tools: fm.tools,
        phases: fm.phases,
        location: None,
        scope: WorkflowScope::User,
        warnings: vec![],
    }
}

#[test]
fn phase_guidance_renders_rules_and_scripts() {
    let wf = sample_workflow();
    let g = phase_guidance(&wf, PHASE_PICK_UP_TASK).expect("has guidance");
    assert!(g.contains("Always reproduce before proposing a fix."));
    assert!(g.contains("git fetch"));
    // Unknown phase => None.
    assert!(phase_guidance(&wf, "on_nonexistent").is_none());
}

#[test]
fn effective_tool_scope_prefers_phase_then_workflow_default() {
    let wf = sample_workflow();
    // pick_up_task overrides with file_read added.
    let pick = effective_tool_scope(&wf, PHASE_PICK_UP_TASK).unwrap();
    assert!(pick.allow.contains(&"file_read".to_string()));
    // enter_directory has no tools => falls back to workflow default.
    let enter = effective_tool_scope(&wf, PHASE_ENTER_DIRECTORY).unwrap();
    assert_eq!(enter.allow, vec!["shell", "git_operations"]);
}

#[test]
fn narrow_visible_tools_intersects_and_denies() {
    let base: HashSet<String> = ["shell", "git_operations", "file_read", "memory_store"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let scope = ToolScope {
        allow: vec![
            "shell".to_string(),
            "git_operations".to_string(),
            "not_in_base".to_string(),
        ],
        deny: vec!["git_operations".to_string()],
    };
    let narrowed = narrow_visible_tools(&base, &scope);
    assert!(narrowed.contains("shell"));
    assert!(!narrowed.contains("git_operations")); // denied
    assert!(!narrowed.contains("not_in_base")); // not in base
    assert!(!narrowed.contains("memory_store")); // not allowed
}

#[test]
fn narrow_with_empty_base_uses_allow_set() {
    let base: HashSet<String> = HashSet::new();
    let scope = ToolScope {
        allow: vec!["shell".to_string()],
        deny: vec![],
    };
    let narrowed = narrow_visible_tools(&base, &scope);
    assert_eq!(narrowed.len(), 1);
    assert!(narrowed.contains("shell"));
}

#[test]
fn best_match_scores_by_when_to_use_overlap() {
    let wf = sample_workflow();
    let workflows = vec![wf];
    let matched = best_match(&workflows, "there is a bug in the broken login").expect("matches");
    assert_eq!(matched.name, "bug-triage");
    // No overlap => None.
    assert!(best_match(&workflows, "schedule a meeting").is_none());
}

// ── inject ───────────────────────────────────────────────────────────────

#[test]
fn render_catalog_lists_workflows_and_is_empty_when_none() {
    assert!(render_workflow_catalog(&[]).is_empty());
    let wf = sample_workflow();
    let cat = render_workflow_catalog(&[wf]);
    assert!(cat.contains("<available_workflows>"));
    assert!(cat.contains("bug-triage"));
    assert!(cat.contains("on_pick_up_task"));
    assert!(cat.contains("workflow_load"));
}
