//! Tests for the collapsed `delegate_to` tool.
//!
//! The load-bearing ones are the two that would let the collapse silently stop
//! paying for itself: `the_envelope_is_emitted_once` (the saving) and
//! `every_member_is_hidden_so_the_collapse_actually_saves_something` (that
//! nothing ships both surfaces).

use super::*;
use crate::openhuman::tools::traits::ToolExposure;

fn targets() -> Vec<DelegateTarget> {
    vec![
        DelegateTarget {
            tool_name: "research".to_string(),
            agent_id: "researcher".to_string(),
            description: "Web research and source gathering.".to_string(),
        },
        DelegateTarget {
            tool_name: "review_code".to_string(),
            agent_id: "code_reviewer".to_string(),
            description: "Review a diff for correctness.".to_string(),
        },
    ]
}

fn tool() -> CollapsedDelegationTool {
    CollapsedDelegationTool::for_targets(targets()).expect("two targets is not empty")
}

#[test]
fn an_empty_target_list_produces_no_tool() {
    // An `agent` enum with no valid value is a schema the model can only call
    // wrongly. Mirrors `SkillDelegationTool::for_connected`.
    assert!(CollapsedDelegationTool::for_targets(Vec::new()).is_none());
}

#[test]
fn the_schema_advertises_every_target() {
    let schema = tool().parameters_schema();
    let listed: Vec<&str> = schema["properties"]["agent"]["enum"]
        .as_array()
        .expect("enum")
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert_eq!(listed, vec!["research", "review_code"]);
}

#[test]
fn the_schema_carries_the_whole_delegation_envelope() {
    // The collapse must not quietly drop a field the members accepted:
    // `render_structured_handoff` reads these exact names, so a missing
    // property is a hand-off that silently loses its constraints.
    let schema = tool().parameters_schema();
    let props = schema["properties"].as_object().expect("properties");
    for field in [
        "prompt",
        "objective",
        "evidence",
        "constraints",
        "must_not_assume",
        "expected_output",
        "citation_requirement",
        "model",
        "blocking",
    ] {
        assert!(props.contains_key(field), "envelope lost `{field}`");
    }
}

#[test]
fn the_envelope_is_emitted_once() {
    // The entire point of the collapse, asserted as a number rather than a
    // shape: one tool holding N targets must not cost what N tools cost.
    //
    // Without this, someone re-introducing a per-target schema (say, to give
    // each specialist its own `expected_output` description) would reproduce
    // the 17,746-byte regression this file exists to remove, and every other
    // test here would still pass.
    let many: Vec<DelegateTarget> = (0..16)
        .map(|i| DelegateTarget {
            tool_name: format!("target_{i}"),
            agent_id: format!("agent_{i}"),
            description: "A specialist.".to_string(),
        })
        .collect();
    let collapsed = CollapsedDelegationTool::for_targets(many).expect("non-empty");
    let bytes = serde_json::to_string(&collapsed.parameters_schema())
        .expect("schema serialises")
        .len();

    // One envelope (~900 B) plus 16 short enum values. Sixteen separate
    // schemas were ~17,700 B; the ceiling here is deliberately far below that
    // and far above the real figure, so it catches a reintroduced fan-out
    // without failing on ordinary wording changes.
    assert!(
        bytes < 2_000,
        "16 targets cost {bytes} B of schema — the envelope is being repeated"
    );
}

#[test]
fn adding_a_target_costs_only_its_name_and_description() {
    // The property that makes the schema constant in the sub-agent dimension:
    // growth must be linear in the *description*, not in the envelope.
    let one = CollapsedDelegationTool::for_targets(vec![DelegateTarget {
        tool_name: "research".to_string(),
        agent_id: "researcher".to_string(),
        description: String::new(),
    }])
    .expect("non-empty");
    let two = CollapsedDelegationTool::for_targets(vec![
        DelegateTarget {
            tool_name: "research".to_string(),
            agent_id: "researcher".to_string(),
            description: String::new(),
        },
        DelegateTarget {
            tool_name: "plan".to_string(),
            agent_id: "planner".to_string(),
            description: String::new(),
        },
    ])
    .expect("non-empty");

    let cost = |t: &CollapsedDelegationTool| {
        serde_json::to_string(&t.parameters_schema())
            .expect("serialises")
            .len()
            + t.description().len()
    };
    // `plan` plus the enum quoting and list punctuation — tens of bytes, not
    // the ~1,100 a whole extra tool schema used to cost.
    assert!(
        cost(&two) - cost(&one) < 60,
        "a second target added {} B",
        cost(&two) - cost(&one)
    );
}

#[test]
fn the_description_carries_each_targets_routing_line() {
    // The routing information is the one thing the collapse must not lose —
    // it is how the model picks a specialist at all.
    let tool = tool();
    let description = tool.description();
    assert!(description.contains("`research`"));
    assert!(description.contains("Web research and source gathering."));
    assert!(description.contains("`review_code`"));
    assert!(description.contains("Review a diff for correctness."));
}

#[test]
fn a_target_with_no_when_to_use_still_lists_its_name() {
    let tool = CollapsedDelegationTool::for_targets(vec![DelegateTarget {
        tool_name: "mystery".to_string(),
        agent_id: "mystery_agent".to_string(),
        description: "   ".to_string(),
    }])
    .expect("non-empty");
    assert!(tool.description().contains("`mystery`"));
    // No dangling ": " when the description is blank.
    assert!(!tool.description().contains("`mystery`: "));
}

#[tokio::test]
async fn an_unknown_agent_is_an_error_naming_the_valid_ones() {
    let result = tool()
        .execute(serde_json::json!({"agent": "researchr", "prompt": "hi"}))
        .await
        .expect("dispatch does not fail the call");
    assert!(result.is_error);
    let text = format!("{result:?}");
    assert!(text.contains("researchr"), "names what was passed: {text}");
    assert!(text.contains("research"), "names the valid ones: {text}");
}

#[tokio::test]
async fn a_missing_agent_is_an_error_rather_than_a_default_route() {
    // Defaulting to the first target would silently send work to the wrong
    // specialist, which is worse than failing.
    let result = tool()
        .execute(serde_json::json!({"prompt": "hi"}))
        .await
        .expect("dispatch does not fail the call");
    assert!(result.is_error);
    assert!(format!("{result:?}").contains("missing"));
}

#[tokio::test]
async fn an_empty_prompt_is_rejected_before_dispatch() {
    let result = tool()
        .execute(serde_json::json!({"agent": "research", "prompt": "   "}))
        .await
        .expect("dispatch does not fail the call");
    assert!(result.is_error);
    assert!(format!("{result:?}").contains("prompt"));
}

#[test]
fn the_timeout_is_unbounded_like_the_members_it_replaces() {
    // Inheriting the 120s single-tool deadline would truncate every sub-agent
    // run that legitimately takes longer — the Sentry regression that put
    // `Unbounded` on `ArchetypeDelegationTool` in the first place.
    assert!(matches!(
        tool().timeout_policy(&serde_json::json!({})),
        crate::openhuman::tools::traits::ToolTimeout::Unbounded
    ));
}

#[test]
fn every_member_is_hidden_so_the_collapse_actually_saves_something() {
    // The load-bearing assertion, matching the one on the collapsed `cron` and
    // `memory` tools: leaving a member `Direct` would ship both surfaces and
    // save nothing, and nothing else in the build would notice.
    let member = super::ArchetypeDelegationTool {
        tool_name: "research".to_string(),
        agent_id: "researcher".to_string(),
        tool_description: "Web research.".to_string(),
    };
    assert_eq!(member.exposure(), ToolExposure::Hidden);
    // …while the collapsed tool itself stays on the wire.
    assert_eq!(tool().exposure(), ToolExposure::Direct);
}

#[test]
fn the_member_and_the_collapsed_tool_agree_on_the_envelope() {
    // Both call `delegation_envelope_properties`, so this pins that neither
    // grew a private copy. A drift here is silent: the collapsed schema would
    // advertise a field `render_structured_handoff` never reads.
    let member = super::ArchetypeDelegationTool {
        tool_name: "research".to_string(),
        agent_id: "researcher".to_string(),
        tool_description: "Web research.".to_string(),
    };
    let member_props = member.parameters_schema()["properties"]
        .as_object()
        .expect("properties")
        .keys()
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();

    let collapsed = tool().parameters_schema();
    let mut collapsed_props = collapsed["properties"]
        .as_object()
        .expect("properties")
        .keys()
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    // The selector is the collapsed tool's own addition.
    assert!(collapsed_props.remove("agent"));

    assert_eq!(member_props, collapsed_props);
}
