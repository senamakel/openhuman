use super::*;
use crate::memory::test_fixtures::{bind_reference, config_in, stored};
use std::collections::HashSet;
use tinyagents_harness::context::{RunConfig, RunContext};
use tinyagents_harness::ids::CallId;
use tinyagents_harness::tool::ToolExecutionContext;
use tinymemory_api::MetaFilter;

#[tokio::test]
async fn the_memory_tool_stops_write_churn_and_recovers_on_the_next_chat_turn() {
    let tmp = tempfile::tempdir().unwrap();
    let config = config_in(&tmp);
    let engine = bind_reference(&config);
    let tool = MemoryTool::new(Arc::new(config.clone()));
    let first: RunContext = RunContext::new(RunConfig::new("first"), ());
    let next: RunContext = RunContext::new(RunConfig::new("next"), ());
    for i in 0..9 {
        let context =
            ToolExecutionContext::from_run_context(&first, CallId::new(format!("call-{i}")));
        let result = tool
            .execute_with_context(
                json!({"action":"learn","text":format!("fact {i}")}),
                ToolCallOptions::default(),
                Some(&context),
            )
            .await
            .unwrap();
        assert_eq!(result.is_error, i == 8);
    }
    super::super::tool_writes::drain(&config).await;
    assert_eq!(stored(&engine, MetaFilter::default()).await.len(), 8);
    let context = ToolExecutionContext::from_run_context(&next, CallId::new("next-call"));
    let result = tool
        .execute_with_context(
            json!({"action":"learn","text":"next turn fact"}),
            ToolCallOptions::default(),
            Some(&context),
        )
        .await
        .unwrap();
    assert!(!result.is_error);
    super::super::tool_writes::drain(&config).await;
    assert_eq!(stored(&engine, MetaFilter::default()).await.len(), 9);
}

#[tokio::test]
async fn harness_runs_have_independent_budgets_and_identical_learnings_deduplicate() {
    use tinyagents_harness::context::{RunConfig, RunContext};
    use tinyagents_harness::ids::CallId;
    use tinyagents_harness::tool::ToolExecutionContext;
    let tmp = tempfile::tempdir().unwrap();
    let config = config_in(&tmp);
    let engine = bind_reference(&config);
    let tool = MemoryTool::new(Arc::new(config.clone()));
    let first: RunContext = RunContext::new(RunConfig::new("first-harness-run"), ());
    let next: RunContext = RunContext::new(RunConfig::new("next-harness-run"), ());
    let args = json!({"action":"learn","text":"Prefers tea"});
    let mut ids = HashSet::new();
    for i in 0..9 {
        let context =
            ToolExecutionContext::from_run_context(&first, CallId::new(format!("call-{i}")));
        let result = tool
            .execute_with_context(args.clone(), ToolCallOptions::default(), Some(&context))
            .await
            .unwrap();
        assert_eq!(result.is_error, i == 8);
        if !result.is_error {
            let value: Value = serde_json::from_str(&result.text()).unwrap();
            ids.insert(value["id"].as_str().unwrap().to_string());
        }
    }
    assert_eq!(
        ids.len(),
        1,
        "invocation IDs must not create duplicate learnings"
    );
    let context = ToolExecutionContext::from_run_context(&next, CallId::new("next-call"));
    let result = tool
        .execute_with_context(args, ToolCallOptions::default(), Some(&context))
        .await
        .unwrap();
    assert!(!result.is_error, "a new harness run gets a fresh budget");
    super::super::tool_writes::drain(&config).await;
    assert_eq!(stored(&engine, MetaFilter::default()).await.len(), 1);
}
