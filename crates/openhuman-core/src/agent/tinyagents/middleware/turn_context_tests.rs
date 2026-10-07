use super::*;
use tinyagents_harness::context::RunConfig;

#[tokio::test]
async fn attachment_scope_is_request_local_and_contains_only_typed_authority() {
    let root = std::path::PathBuf::from("/workspace/project");
    let mut data = crate::agent::tinyagents::host::OpenHumanRunContext::new();
    data.origin = Some(
        crate::agent::turn_origin::AgentTurnOrigin::ExternalChannel {
            channel: "test".into(),
            sender: None,
            reply_target: "room".into(),
            message_id: "message".into(),
            history_key: None,
        },
    );
    data.workspace = Some(tinytools::WorkspaceDescriptor::new(root.clone()));
    let mut context = RunContext::new(RunConfig::new("attachment-scope"), data);
    let mut request = ModelRequest::new(Vec::new());
    request.metadata["preserved"] = serde_json::json!(true);

    AttachmentRequestScopeMiddleware
        .before_model(&mut context, &(), &mut request)
        .await
        .expect("scope carrier is installed");

    let mut provider_metadata = request.metadata.clone();
    let scope = crate::agent::attachments::take_request_scope(&mut provider_metadata);
    assert!(scope.external_channel);
    assert_eq!(scope.workspace.as_deref(), Some(root.as_path()));
    assert_eq!(provider_metadata, serde_json::json!({"preserved": true}));
    assert!(request.messages.is_empty());
}

#[tokio::test]
async fn attachment_scope_observes_even_after_an_earlier_middleware_stops() {
    assert!(AttachmentRequestScopeMiddleware.is_observer());
}

#[tokio::test]
async fn transcript_snapshot_keeps_a_completed_failed_tool_row() {
    let sink = Arc::new(std::sync::Mutex::new(TranscriptSnapshot::default()));
    let middleware = TranscriptSnapshotMiddleware {
        sink: sink.clone(),
        started: Default::default(),
    };
    let mut context = RunContext::new(
        RunConfig::new("snapshot-tool-outcome"),
        crate::agent::tinyagents::host::OpenHumanRunContext::new(),
    );
    let mut call = tinyinference_llm::tool::ToolCall {
        id: "failed-call".into(),
        name: "write_file".into(),
        arguments: serde_json::json!({"path": "blocked.txt"}),
        invalid: None,
    };
    middleware
        .before_tool(&mut context, &(), &mut call)
        .await
        .expect("snapshot accepts tool start");
    let invocation = ToolInvocationIdentity::new("failed-call", "write_file");
    let mut result = TaToolResult::error("permission denied");
    middleware
        .after_tool(&mut context, &(), &invocation, &mut result)
        .await
        .expect("snapshot accepts tool completion");

    let snapshot = sink.lock().expect("snapshot");
    assert_eq!(
        snapshot.messages,
        vec![Message::tool("failed-call", "permission denied")]
    );
    assert_eq!(snapshot.tool_outcomes.len(), 1);
    let outcome = &snapshot.tool_outcomes[0];
    assert_eq!(outcome.call_id, "failed-call");
    assert_eq!(outcome.name, "write_file");
    assert_eq!(
        outcome.arguments,
        serde_json::json!({"path": "blocked.txt"})
    );
    assert!(!outcome.success);
    assert_eq!(outcome.content, "permission denied");
}
