#[tokio::test]
async fn unknown_tool_call_projects_exactly_one_failed_timeline_row() {
    // Production Langfuse showed two ERROR tool observations ~15ms apart for
    // every unknown-tool call: the bridge synthesised a Started/Completed pair
    // off `UnknownToolCall` (null output, NotFound copy) and the crate's own
    // recovered pair produced a second one carrying the error text. Exactly
    // one row per call, carrying the error text and the NotFound class.
    let (tx, mut rx) = tokio::sync::mpsc::channel(64);
    let bridge = OpenhumanEventBridge::new(Some(tx), "mock-model", 10);
    let sink = EventSink::new();
    sink.subscribe(bridge.clone());

    emit_recovered_unknown_tool_call(&sink, "c9");

    let mut started = Vec::new();
    let mut completed = Vec::new();
    while let Ok(p) = rx.try_recv() {
        match p {
            AgentProgress::ToolCallStarted {
                tool_name,
                display_label,
                ..
            } => started.push((tool_name, display_label)),
            AgentProgress::ToolCallCompleted {
                tool_name,
                success,
                failure,
                output,
                display_label,
                ..
            } => completed.push((tool_name, success, failure, output, display_label)),
            _ => {}
        }
    }
    assert_eq!(started.len(), 1, "exactly one started row: {started:?}");
    assert_eq!(completed.len(), 1, "exactly one completed row");
    assert_eq!(started[0].0, "search_files");
    assert_eq!(
        started[0].1.as_deref(),
        Some("Search Files (unavailable)"),
        "the timeline still says the tool was unavailable"
    );
    let (tool_name, success, failure, output, label) = completed.remove(0);
    assert_eq!(tool_name, "search_files");
    assert!(!success, "the attempted tool is projected as a failed call");
    assert_eq!(
        output, UNKNOWN_TOOL_ANSWER,
        "the row carries the error text"
    );
    assert_eq!(label.as_deref(), Some("Search Files (unavailable)"));
    // #6277: a tool the agent does not have fails identically on every retry,
    // so the timeline must not tell the user to "try again / run diagnostics".
    let failure = failure.expect("the failed row carries a classified failure");
    assert_eq!(
        failure.class,
        tinytools::status::ToolFailureClass::NotFound,
        "an unavailable tool must be classified NotFound, not Unknown"
    );
    assert!(!failure.recoverable);
}

#[tokio::test]
async fn unknown_tool_call_alone_projects_no_tool_row() {
    // The typed event only annotates the call; the crate's recovered pair is
    // what opens and closes the row.
    let (tx, mut rx) = tokio::sync::mpsc::channel(64);
    let bridge = OpenhumanEventBridge::new(Some(tx), "mock-model", 10);
    let sink = EventSink::new();
    sink.subscribe(bridge.clone());

    sink.emit(AgentEvent::UnknownToolCall {
        call_id: "c9".into(),
        requested_name: "search_files".to_string(),
        arguments: serde_json::json!({}),
        recovery: "tool_error".to_string(),
    });
    while let Ok(p) = rx.try_recv() {
        assert!(
            !matches!(
                p,
                AgentProgress::ToolCallStarted { .. } | AgentProgress::ToolCallCompleted { .. }
            ),
            "UnknownToolCall must not synthesise its own tool row"
        );
    }
}

#[tokio::test]
async fn unknown_tool_call_in_a_child_run_projects_one_subagent_row() {
    let (tx, mut rx) = tokio::sync::mpsc::channel(64);
    let bridge = OpenhumanEventBridge::with_scope(
        Some(tx),
        "mock-model",
        "managed",
        10,
        Some(super::SubagentScope {
            agent_id: "researcher".to_string(),
            task_id: "task-1".to_string(),
            extended_policy: false,
        }),
        Arc::default(),
        Arc::default(),
        Arc::default(),
        Arc::default(),
        Vec::new(),
        crate::core::runtime::DispatchCtx::ambient(),
    );
    let sink = EventSink::new();
    sink.subscribe(bridge.clone());

    emit_recovered_unknown_tool_call(&sink, "c10");

    let (mut started, mut completed) = (0, 0);
    let mut class = None;
    while let Ok(p) = rx.try_recv() {
        match p {
            AgentProgress::SubagentToolCallStarted { .. } => started += 1,
            AgentProgress::SubagentToolCallCompleted { failure, .. } => {
                completed += 1;
                class = failure.map(|f| f.class);
            }
            _ => {}
        }
    }
    assert_eq!((started, completed), (1, 1));
    assert_eq!(class, Some(tinytools::status::ToolFailureClass::NotFound));
}

/// W2-budget-dedupe: two `UsageRecorded` events for the *same* model call
/// (as happens once the observe-only crate `BudgetMiddleware` re-emits usage
/// its `after_model` folded, on top of the runtime's own emit) must be
/// recorded into the bridge accounting **exactly once**. Without the dedupe
/// guard the totals would double.
