use super::*;

use std::sync::atomic::{AtomicBool, Ordering};

use tinytools::{ToolContent, ToolPolicy};

struct RecordingTool {
    saw_context: AtomicBool,
}

#[async_trait]
impl Tool for RecordingTool {
    fn name(&self) -> &str {
        "recording"
    }

    fn description(&self) -> &str {
        "Preserves the canonical TinyTools contract."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({"type": "object", "properties": {"fail": {"type": "boolean"}}})
    }

    fn policy(&self) -> ToolPolicy {
        ToolPolicy::read_only().requiring_approval()
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        self.execute_with_context(args, ToolCallOptions::default(), None)
            .await
    }

    async fn execute_with_context(
        &self,
        args: serde_json::Value,
        options: ToolCallOptions,
        context: Option<&dyn ToolRunContext>,
    ) -> anyhow::Result<ToolResult> {
        self.saw_context.store(
            context.is_some() && options.prefer_markdown,
            Ordering::SeqCst,
        );
        Ok(ToolResult {
            content: vec![
                ToolContent::Text {
                    text: "plain content".to_string(),
                },
                ToolContent::Json {
                    data: serde_json::json!({"preserved": true}),
                },
            ],
            is_error: args["fail"].as_bool().unwrap_or(false),
            markdown_formatted: Some("markdown content".to_string()),
            ..ToolResult::default()
        })
    }
}

struct TestToolRunContext;
impl ToolRunContext for TestToolRunContext {}

#[tokio::test]
async fn canonical_adapter_preserves_spec_policy_context_and_result() {
    let seen_context = Arc::new(AtomicBool::new(false));
    struct SharedRecordingTool(Arc<AtomicBool>);
    #[async_trait]
    impl Tool for SharedRecordingTool {
        fn name(&self) -> &str {
            "recording"
        }
        fn description(&self) -> &str {
            "Preserves the canonical TinyTools contract."
        }
        fn parameters_schema(&self) -> serde_json::Value {
            serde_json::json!({"type": "object"})
        }
        fn policy(&self) -> ToolPolicy {
            ToolPolicy::read_only().requiring_approval()
        }
        async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
            self.execute_with_context(args, ToolCallOptions::default(), None)
                .await
        }
        async fn execute_with_context(
            &self,
            args: serde_json::Value,
            options: ToolCallOptions,
            context: Option<&dyn ToolRunContext>,
        ) -> anyhow::Result<ToolResult> {
            self.0.store(
                context.is_some() && options.prefer_markdown,
                Ordering::SeqCst,
            );
            Ok(ToolResult {
                content: vec![
                    ToolContent::Text {
                        text: "plain content".into(),
                    },
                    ToolContent::Json {
                        data: serde_json::json!({"preserved": true}),
                    },
                ],
                is_error: args["fail"].as_bool().unwrap_or(false),
                markdown_formatted: Some("markdown content".into()),
                ..ToolResult::default()
            })
        }
    }
    let sets: Vec<Arc<Vec<Box<dyn Tool>>>> = vec![Arc::new(vec![Box::new(SharedRecordingTool(
        seen_context.clone(),
    ))])];
    let adapter = CanonicalSharedToolAdapter::for_name(sets, "recording").expect("registered tool");

    assert_eq!(adapter.name(), "recording");
    assert_eq!(
        adapter.description(),
        "Preserves the canonical TinyTools contract."
    );
    assert_eq!(
        adapter.parameters_schema(),
        serde_json::json!({"type": "object"})
    );
    assert_eq!(
        adapter.policy(),
        ToolPolicy::read_only().requiring_approval()
    );

    let result = adapter
        .execute_with_context(
            serde_json::json!({"fail": true}),
            ToolCallOptions::prefer_markdown(),
            Some(&TestToolRunContext),
        )
        .await
        .expect("canonical result");
    assert!(seen_context.load(Ordering::SeqCst));
    assert!(result.is_error);
    assert_eq!(
        result.markdown_formatted.as_deref(),
        Some("markdown content")
    );
    assert_eq!(result.content.len(), 2);
    assert!(matches!(result.content[0], ToolContent::Text { ref text } if text == "plain content"));
    assert!(
        matches!(result.content[1], ToolContent::Json { ref data } if data == &serde_json::json!({"preserved": true}))
    );
}

struct OriginObservingTool(Arc<AtomicBool>);

#[async_trait]
impl Tool for OriginObservingTool {
    fn name(&self) -> &str {
        "origin_observer"
    }
    fn description(&self) -> &str {
        "Observes the per-turn core authority."
    }
    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({"type":"object"})
    }
    async fn execute(&self, _args: serde_json::Value) -> anyhow::Result<ToolResult> {
        Ok(ToolResult::success("executed"))
    }
    async fn execute_with_context(
        &self,
        _args: serde_json::Value,
        _options: ToolCallOptions,
        _context: Option<&dyn ToolRunContext>,
    ) -> anyhow::Result<ToolResult> {
        self.0.store(
            matches!(
                crate::core::runtime::CoreContext::current_turn_origin(),
                Some(crate::agent::turn_origin::AgentTurnOrigin::ExternalChannel { .. })
            ),
            Ordering::SeqCst,
        );
        Ok(ToolResult::success("executed"))
    }
}

#[tokio::test]
async fn canonical_adapter_scopes_tool_execution_to_the_run_origin() {
    use tinyagents_harness::context::RunConfig;

    let context =
        crate::core::runtime::CoreContext::for_test(crate::core::runtime::DomainSet::full(), None);
    let observed = Arc::new(AtomicBool::new(false));
    let tools: Vec<Arc<Vec<Box<dyn Tool>>>> = vec![Arc::new(vec![Box::new(OriginObservingTool(
        observed.clone(),
    ))])];
    let adapter = CanonicalSharedToolAdapter::for_name(tools, "origin_observer").unwrap();
    let mut host = crate::agent::tinyagents::host::OpenHumanRunContext::new();
    host.origin = Some(
        crate::agent::turn_origin::AgentTurnOrigin::ExternalChannel {
            channel: "test".into(),
            sender: None,
            reply_target: "room".into(),
            message_id: "message".into(),
            history_key: None,
        },
    );
    let run = host.into_tinyagents(RunConfig::new("origin-observer"));
    let tool_context = tinyagents_harness::tool::ToolExecutionContext::from_run_context(
        &run,
        tinyagents_harness::ids::CallId::new("call"),
    );

    crate::core::runtime::CoreContext::scope(context, async {
        adapter
            .execute_with_context(
                serde_json::json!({}),
                ToolCallOptions::default(),
                Some(&tool_context),
            )
            .await
            .expect("tool execution succeeds");
        assert!(observed.load(Ordering::SeqCst));
        assert!(crate::core::runtime::CoreContext::current_turn_origin().is_none());
    })
    .await;
}

#[tokio::test]
async fn canonical_adapter_fails_closed_when_the_registered_tool_is_gone() {
    let adapter = CanonicalSharedToolAdapter {
        sets: Vec::new(),
        name: "missing".to_string(),
        description: "missing".to_string(),
        parameters_schema: serde_json::json!({"type": "object"}),
        early_exit: None,
        force_deferred: false,
    };

    let result = adapter
        .execute(serde_json::json!({}))
        .await
        .expect("reported failure");
    assert!(result.is_error);
    assert_eq!(result.output(), "unknown tool 'missing'");
}

#[tokio::test]
async fn early_exit_only_fires_after_a_successful_canonical_result() {
    let sets: Vec<Arc<Vec<Box<dyn Tool>>>> = vec![Arc::new(vec![Box::new(RecordingTool {
        saw_context: AtomicBool::new(false),
    })])];
    let hook = EarlyExitHook::new(SteeringHandle::allow_all());
    let adapter = CanonicalSharedToolAdapter::for_name(sets, "recording")
        .expect("registered tool")
        .with_early_exit(hook.clone());

    let failed = adapter
        .execute(serde_json::json!({"fail": true}))
        .await
        .expect("reported failure");
    assert!(failed.is_error);
    assert!(
        hook.take().is_none(),
        "reported tool errors must not pause the run"
    );

    let successful = adapter
        .execute(serde_json::json!({"fail": false}))
        .await
        .expect("success");
    assert!(!successful.is_error);
    let early_exit = hook
        .take()
        .expect("successful early-exit tool pauses the run");
    assert_eq!(early_exit.tool, "recording");
    assert_eq!(early_exit.question, "markdown content");
}

struct ExposedTool(&'static str, tinytools::ToolExposure);

#[async_trait]
impl Tool for ExposedTool {
    fn name(&self) -> &str {
        self.0
    }

    fn description(&self) -> &str {
        "exposure probe"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({"type": "object"})
    }

    fn exposure(&self) -> tinytools::ToolExposure {
        self.1
    }

    async fn execute(&self, _args: serde_json::Value) -> anyhow::Result<ToolResult> {
        Ok(ToolResult::default())
    }
}

/// The host decides what a belt advertises; the harness only needs to know
/// which admitted registrations are searchable rather than advertised. A
/// `Hidden` tool that reached registration was named by the belt, so it is
/// advertised; `Deferred` stays deferred (#6370).
#[test]
fn adapter_advertises_admitted_hidden_tools_and_keeps_deferred_ones_deferred() {
    let set: Arc<Vec<Box<dyn Tool>>> = Arc::new(vec![
        Box::new(ExposedTool("direct", tinytools::ToolExposure::Direct)),
        Box::new(ExposedTool("hidden", tinytools::ToolExposure::Hidden)),
        Box::new(ExposedTool("deferred", tinytools::ToolExposure::Deferred)),
    ]);
    let exposure = |name: &str| {
        CanonicalSharedToolAdapter::for_name(vec![set.clone()], name)
            .expect("registered")
            .exposure()
    };
    assert_eq!(exposure("direct"), tinytools::ToolExposure::Direct);
    assert_eq!(exposure("hidden"), tinytools::ToolExposure::Direct);
    assert_eq!(exposure("deferred"), tinytools::ToolExposure::Deferred);
}

/// A tool the session defers for its own agent (`deferred_tools` →
/// `OpenHumanRunContext::deferred_tool_names`) registers as `Deferred`, so the
/// harness keeps it off the wire and indexes it for `tool_search`. Deferral only
/// subtracts: a forced `Hidden` tool stays `Hidden` instead of becoming
/// searchable, and an already-`Deferred` tool is unchanged.
#[test]
fn session_deferred_adapter_reports_deferred_but_never_surfaces_a_hidden_tool() {
    let set: Arc<Vec<Box<dyn Tool>>> = Arc::new(vec![
        Box::new(ExposedTool("direct", tinytools::ToolExposure::Direct)),
        Box::new(ExposedTool("hidden", tinytools::ToolExposure::Hidden)),
        Box::new(ExposedTool("deferred", tinytools::ToolExposure::Deferred)),
    ]);
    let forced = |name: &str| {
        CanonicalSharedToolAdapter::for_name(vec![set.clone()], name)
            .expect("registered")
            .deferred()
            .exposure()
    };
    assert_eq!(forced("direct"), tinytools::ToolExposure::Deferred);
    assert_eq!(forced("hidden"), tinytools::ToolExposure::Hidden);
    assert_eq!(forced("deferred"), tinytools::ToolExposure::Deferred);
}
