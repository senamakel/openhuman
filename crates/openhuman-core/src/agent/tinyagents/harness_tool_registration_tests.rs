//! Coverage for [`register_turn_tools_and_agents`]'s thread-scoped filtering.

use super::*;

/// Register the real goal tools on a fresh harness and return the candidate
/// names handed on to the shadow tool-exposure layer.
fn candidate_names(has_thread: bool) -> Vec<String> {
    let workspace = tempfile::TempDir::new().expect("workspace");
    let tools = crate::agent::goals::goal_tools(workspace.path());
    let mut harness: AgentHarness<(), OpenHumanRunContext> = AgentHarness::new();
    let mut registry: CapabilityRegistry<()> = CapabilityRegistry::new();

    let (tool_count, candidates, _, _) = register_turn_tools_and_agents(
        &mut harness,
        &mut registry,
        &[Arc::new(tools)],
        &None,
        &HashSet::new(),
        None,
        false,
        has_thread,
        &HashSet::new(),
    );
    assert_eq!(tool_count, harness.tools().names().len());
    candidates
}

#[test]
fn thread_less_turn_drops_goal_tools_from_the_exposure_candidates() {
    // The shadow exposure layer diffs these against the registered tools; a
    // candidate that is never registered would log a false divergence warn.
    let candidates = candidate_names(false);

    assert!(
        candidates.iter().all(|name| !is_thread_goal_tool(name)),
        "goal tools must not be exposure candidates without a thread: {candidates:?}"
    );
}

#[test]
fn threaded_turn_keeps_goal_tools_as_exposure_candidates() {
    let candidates = candidate_names(true);

    for name in ["goal_get", "goal_set", "goal_complete"] {
        assert!(
            candidates.iter().any(|candidate| candidate == name),
            "{name} must stay a candidate on a threaded turn: {candidates:?}"
        );
    }
}

#[tokio::test]
async fn registered_memory_dispatch_enforces_one_budget_across_distinct_calls() {
    use crate::memory::test_fixtures::{bind_reference, config_in};
    use tinyagents_harness::{context::RunConfig, CallId};
    use tinytools::ToolCallOptions;
    let workspace = tempfile::tempdir().unwrap();
    let config = config_in(&workspace);
    let _engine = bind_reference(&config);
    let tools: Arc<Vec<Box<dyn tinytools::Tool>>> = Arc::new(vec![Box::new(
        crate::memory::tools::MemoryTool::new(Arc::new(config.clone())),
    )]);
    let adapter = CanonicalSharedToolAdapter::for_name(vec![tools], "memory").unwrap();
    let dispatch = typed_dispatch_for("memory", Arc::new(adapter))
        .expect("memory registration must preserve the harness run context");
    let parent = OpenHumanRunContext::new().into_tinyagents(RunConfig::new("memory-dispatch"));
    for index in 0..9 {
        let result = dispatch
            .execute(
                &(),
                CallId::new(format!("call-{index}")),
                serde_json::json!({"action":"learn","text":format!("fact {index}")}),
                ToolCallOptions::default(),
                &parent,
            )
            .await
            .unwrap();
        assert_eq!(result.is_error, index == 8, "call {index}");
    }
    crate::memory::tool_writes::drain(&config).await;
}
