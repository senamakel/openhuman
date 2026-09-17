use super::*;

fn echo_tool_call() -> anyhow::Result<ChatResponse> {
    Ok(ChatResponse {
        text: Some("<tool_call>{\"name\":\"echo\",\"arguments\":{}}</tool_call>".into()),
        tool_calls: vec![],
        usage: Some(UsageInfo {
            input_tokens: 100,
            output_tokens: 7,
            ..UsageInfo::default()
        }),
        reasoning_content: None,
    })
}

fn failed_turn_agent(provider: Arc<dyn ChatModel<()>>) -> Agent {
    make_agent_with_builder(
        provider,
        vec![Box::new(EchoTool)],
        vec![],
        crate::config::AgentConfig {
            max_tool_iterations: 5,
            ..crate::config::AgentConfig::default()
        },
        crate::config::ContextConfig::default(),
    )
}

fn tool_call_rounds(agent: &Agent) -> usize {
    agent
        .history
        .iter()
        .filter(|message| matches!(message, ConversationMessage::AssistantToolCalls { .. }))
        .count()
}

/// #6281: a turn that fails after doing work keeps the rounds the provider
/// accepted plus the failure cause, persists them, and the next turn sees them.
#[tokio::test]
async fn failed_turn_keeps_its_work_and_cause_for_the_next_turn() {
    let provider_impl = Arc::new(SequenceProvider {
        responses: AsyncMutex::new(vec![
            // Call 1 answered: round 1 runs.
            echo_tool_call(),
            // Call 2 answered (its request carried round 1): round 2 runs.
            echo_tool_call(),
            // Call 3 fails; only its request carried round 2. A 400 is not
            // retried by the harness, so the turn fails on this one response.
            Err(anyhow::anyhow!("400 Bad Request: provider boom")),
            // The follow-up turn.
            Ok(ChatResponse {
                text: Some("The last turn hit a provider error.".into()),
                tool_calls: vec![],
                usage: None,
                reasoning_content: None,
            }),
        ]),
        requests: AsyncMutex::new(Vec::new()),
        tool_counts: AsyncMutex::new(Vec::new()),
    });
    let mut agent = failed_turn_agent(provider_impl.clone());

    let err = agent
        .turn("run the task")
        .await
        .expect_err("the third model call fails the turn");
    assert!(err.to_string().contains("provider boom"), "got: {err}");

    // Round 1 was in a request the provider answered, so it is replayed as
    // structured messages. Round 2 was only in the failing request, so it must
    // not be (a rejected request must never be replayed), and lives in the note.
    assert_eq!(
        tool_call_rounds(&agent),
        1,
        "only the accepted round is kept as structured tool calls: {:?}",
        agent.history
    );
    assert!(agent
        .history
        .iter()
        .any(|message| matches!(message, ConversationMessage::ToolResults(results) if !results.is_empty())));
    let note = match agent.history.last() {
        Some(ConversationMessage::Chat(chat)) if chat.role == "assistant" => chat.content.clone(),
        other => panic!("a failed turn must end history on its failure note, got: {other:?}"),
    };
    assert!(
        note.starts_with("[turn failed before completion:") && note.contains("provider boom"),
        "the note must carry the failure cause, got: {note}"
    );
    assert!(
        note.contains("called `echo`"),
        "the unanswered round must be kept as text, got: {note}"
    );

    let path = agent
        .session_transcript_path
        .clone()
        .expect("a failed turn must persist its transcript");
    let persisted = transcript::read_transcript(&path).expect("read transcript");
    assert!(
        persisted
            .messages
            .iter()
            .any(|message| message.role == "assistant" && message.content.contains("provider boom")),
        "the persisted transcript must carry the failure note"
    );
    // Both answered calls reported usage before the failure; the failed turn
    // records it rather than zeros.
    assert_eq!(
        (persisted.meta.input_tokens, persisted.meta.output_tokens),
        (200, 14),
        "the failed turn must record the usage of its answered calls"
    );

    agent
        .turn("what happened?")
        .await
        .expect("the follow-up turn succeeds");
    let requests = provider_impl.requests.lock().await;
    let follow_up = requests.last().expect("follow-up request");
    assert!(
        follow_up
            .iter()
            .any(|message| message.content.starts_with("[Tool results]")),
        "the follow-up request must carry the failed turn's tool result: {follow_up:?}"
    );
    assert!(
        follow_up
            .iter()
            .any(|message| message.role == "assistant" && message.content.contains("provider boom")),
        "the follow-up request must carry the failure cause: {follow_up:?}"
    );
}

/// #6281: a turn that fails on its first model call still records why, so the
/// history never holds two user messages in a row with no explanation between.
#[tokio::test]
async fn turn_failing_on_first_call_records_the_cause() {
    let provider: Arc<dyn ChatModel<()>> = Arc::new(SequenceProvider {
        responses: AsyncMutex::new(vec![Err(anyhow::anyhow!("400 Bad Request: provider boom"))]),
        requests: AsyncMutex::new(Vec::new()),
        tool_counts: AsyncMutex::new(Vec::new()),
    });
    let mut agent = failed_turn_agent(provider);

    agent
        .turn("run the task")
        .await
        .expect_err("the first model call fails the turn");

    assert_eq!(tool_call_rounds(&agent), 0);
    let tail: Vec<(String, String)> = agent
        .history
        .iter()
        .rev()
        .take(2)
        .map(|message| match message {
            ConversationMessage::Chat(chat) => (chat.role.clone(), chat.content.clone()),
            other => panic!("unexpected history entry: {other:?}"),
        })
        .collect();
    assert_eq!(tail[1].0, "user");
    assert_eq!(tail[0].0, "assistant");
    assert!(tail[0].1.contains("provider boom"), "got: {}", tail[0].1);
    assert!(agent.session_transcript_path.is_some());
}

/// #6281 review: an error in the tool stage, after a model response and before
/// the next model request, still records that response's tool calls.
#[tokio::test]
async fn tool_stage_failure_keeps_the_latest_response() {
    // One response asking for more tool calls than the turn's tool-call cap
    // (eight per allowed model call): admission of the call past the cap fails
    // the run in the tool stage, before any further model request.
    let calls: String = (0..9)
        .map(|n| {
            format!("<tool_call>{{\"name\":\"echo\",\"arguments\":{{\"n\":{n}}}}}</tool_call>")
        })
        .collect();
    let provider: Arc<dyn ChatModel<()>> = Arc::new(SequenceProvider {
        responses: AsyncMutex::new(vec![Ok(ChatResponse {
            text: Some(calls),
            tool_calls: vec![],
            usage: None,
            reasoning_content: None,
        })]),
        requests: AsyncMutex::new(Vec::new()),
        tool_counts: AsyncMutex::new(Vec::new()),
    });
    let mut agent = make_agent_with_builder(
        provider,
        vec![Box::new(EchoTool)],
        vec![],
        crate::config::AgentConfig {
            max_tool_iterations: 1,
            ..crate::config::AgentConfig::default()
        },
        crate::config::ContextConfig::default(),
    );

    let err = agent
        .turn("run the task")
        .await
        .expect_err("the call past the tool-call cap fails the turn");

    let note = match agent.history.last() {
        Some(ConversationMessage::Chat(chat)) if chat.role == "assistant" => chat.content.clone(),
        other => panic!("a failed turn must end history on its failure note, got: {other:?}"),
    };
    assert!(
        note.contains("called `echo`"),
        "the response the tool stage failed on must be kept as text ({err}), got: {note}"
    );
    assert_eq!(
        tool_call_rounds(&agent),
        0,
        "a response no provider has seen again is never replayed as structured tool calls"
    );
}
