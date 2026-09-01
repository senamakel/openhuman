//! Tests for the flow → catalogue-entry mapping.

use super::*;
use crate::openhuman::flows::types::Flow;
use tinyflows::model::{Node, NodeKind, WorkflowGraph};

fn node(id: &str, kind: NodeKind, config: serde_json::Value) -> Node {
    Node {
        id: id.into(),
        kind,
        name: id.to_string(),
        config,
        type_version: 1,
        ports: Vec::new(),
        position: None,
    }
}

fn flow(name: &str, enabled: bool, nodes: Vec<Node>) -> Flow {
    Flow {
        id: "flow-abc-123".to_string(),
        name: name.to_string(),
        enabled,
        graph: WorkflowGraph {
            nodes,
            ..Default::default()
        },
        created_at: String::new(),
        updated_at: String::new(),
        last_run_at: None,
        last_status: None,
        require_approval: false,
    }
}

#[test]
fn the_entry_is_keyed_by_the_flow_id_not_a_slug_of_its_name() {
    // `run_workflow` and `get_flow` take the id. A slug of the display name
    // would be a second identifier that resolves nowhere, and the model would
    // have no way to tell which one it was holding.
    let entry = entry_for(flow("Morning Digest", true, vec![]));
    assert_eq!(entry.dir_name, "flow-abc-123");
    assert_eq!(entry.name, "Morning Digest");
}

#[test]
fn the_entry_is_scoped_flow_and_carries_no_on_disk_bundle() {
    // The distinction that makes `WorkflowScope::Flow` worth having: consumers
    // that read `location` or `resources` must get an honest absence rather
    // than a path that does not exist.
    let entry = entry_for(flow("Anything", true, vec![]));
    assert_eq!(entry.scope, WorkflowScope::Flow);
    assert!(entry.location.is_none());
    assert!(entry.resources.is_empty());
}

#[test]
fn the_description_names_the_trigger_and_step_count() {
    let f = flow(
        "Digest",
        true,
        vec![
            node(
                "t",
                NodeKind::Trigger,
                serde_json::json!({ "trigger_kind": "schedule" }),
            ),
            node("a", NodeKind::Agent, serde_json::json!({})),
            node("b", NodeKind::Agent, serde_json::json!({})),
        ],
    );
    let entry = entry_for(f);
    assert!(
        entry.description.contains("schedule"),
        "{}",
        entry.description
    );
    // Two steps: the trigger is not a step a user thinks about.
    assert!(
        entry.description.contains("2 steps"),
        "{}",
        entry.description
    );
}

#[test]
fn a_single_step_is_not_pluralised() {
    let f = flow(
        "One",
        true,
        vec![
            node(
                "t",
                NodeKind::Trigger,
                serde_json::json!({ "trigger_kind": "manual" }),
            ),
            node("a", NodeKind::Agent, serde_json::json!({})),
        ],
    );
    assert!(entry_for(f).description.contains("1 step)"));
}

#[test]
fn a_graph_with_no_trigger_reads_as_manual_rather_than_blank() {
    // `graph.trigger()` also returns `None` when there are *several* triggers.
    // Either way the catalogue must say something; validation reports the real
    // problem separately, so this stays quiet rather than duplicating it.
    let entry = entry_for(flow("Headless", true, vec![]));
    assert!(
        entry.description.contains("manual"),
        "{}",
        entry.description
    );
}

#[test]
fn a_disabled_flow_is_still_listed_and_says_it_is_paused() {
    // Hiding it would make the model answer "you have no such automation" to
    // someone looking straight at it in the UI.
    let entry = entry_for(flow("Paused", false, vec![]));
    assert!(entry.description.contains("Currently disabled"));
}

#[test]
fn an_enabled_flow_does_not_claim_to_be_disabled() {
    assert!(!entry_for(flow("Live", true, vec![]))
        .description
        .contains("disabled"));
}

#[test]
fn the_synthesised_description_never_claims_to_know_the_purpose() {
    // Pinning the module's own rule. `Flow` carries no description field, so
    // anything purpose-shaped here would be invented. If a `description` lands
    // on the flow model, this test should be replaced by one asserting it is
    // used — not deleted quietly.
    let entry = entry_for(flow("Send invoices to accounting", true, vec![]));
    assert!(
        entry.description.starts_with("Saved Flows automation"),
        "description must describe the shape, not guess the intent: {}",
        entry.description
    );
    assert!(!entry.description.contains("invoice"));
}
