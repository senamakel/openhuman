//! Issues #6278 and #6279: a turn with no usable reply of its own is closed from
//! its tool records, and the closing message is checked before it is shown.

use super::*;
use crate::agent::progress::AgentProgress;

/// What the failing tool returns. It carries a link on purpose: these defects
/// dropped exactly this kind of user-actionable detail.
const INSTALL_FAILURE: &str = "item 'demo' has no direct download, so it can't be installed \
                               automatically. View it at https://example.test/demo";

const INSTALL_CALL: &str = "<tool_call>{\"name\":\"install_item\",\"arguments\":{}}</tool_call>";

struct FailingInstallTool;

#[async_trait]
impl Tool for FailingInstallTool {
    fn name(&self) -> &str {
        "install_item"
    }

    fn description(&self) -> &str {
        "install an item"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({"type":"object"})
    }

    async fn execute(&self, _args: serde_json::Value) -> Result<ToolResult> {
        Ok(ToolResult::error(INSTALL_FAILURE))
    }
}

/// Succeeds on its first call and fails on every later one, so each tool round's
/// record is distinguishable from the others.
struct TwoRoundInstallTool {
    calls: AtomicUsize,
}

#[async_trait]
impl Tool for TwoRoundInstallTool {
    fn name(&self) -> &str {
        "install_item"
    }

    fn description(&self) -> &str {
        "install an item"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({"type":"object"})
    }

    async fn execute(&self, _args: serde_json::Value) -> Result<ToolResult> {
        if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
            Ok(ToolResult::success(
                "round one: found the demo item in the catalog",
            ))
        } else {
            Ok(ToolResult::error(INSTALL_FAILURE))
        }
    }
}

fn respond(text: &str) -> anyhow::Result<ChatResponse> {
    Ok(ChatResponse {
        text: Some(text.into()),
        ..ChatResponse::default()
    })
}

fn scripted(responses: Vec<anyhow::Result<ChatResponse>>) -> Arc<SequenceProvider> {
    Arc::new(SequenceProvider {
        responses: AsyncMutex::new(responses),
        requests: AsyncMutex::new(Vec::new()),
        tool_counts: AsyncMutex::new(Vec::new()),
    })
}

/// Run one turn against `provider`, returning the reply and everything streamed.
async fn run_turn(
    provider: Arc<SequenceProvider>,
    tools: Vec<Box<dyn Tool>>,
) -> (Agent, String, String) {
    let mut agent = make_agent_with_builder(
        provider,
        tools,
        vec![],
        crate::config::AgentConfig {
            max_tool_iterations: 8,
            ..crate::config::AgentConfig::default()
        },
        crate::config::ContextConfig::default(),
    );
    let (progress_tx, mut progress_rx) = tokio::sync::mpsc::channel(256);
    agent.set_on_progress(Some(progress_tx));
    let reply = agent
        .turn("install the demo item")
        .await
        .expect("the turn should close with a reply, not error");
    agent.set_on_progress(None);
    let mut streamed = String::new();
    while let Ok(progress) = progress_rx.try_recv() {
        if let AgentProgress::TextDelta { delta, .. } = progress {
            streamed.push_str(&delta);
        }
    }
    (agent, reply, streamed)
}

fn history_ends_on(agent: &Agent, reply: &str) -> bool {
    matches!(
        agent.history.last(),
        Some(ConversationMessage::Chat(msg)) if msg.role == "assistant" && msg.content == reply
    )
}

#[tokio::test]
async fn an_intent_only_close_is_rejected_and_replaced_by_the_tool_records() {
    const NARRATION: &str = "I'll search the registry for the demo item.";
    let recorded = scripted(vec![
        respond(INSTALL_CALL),
        // The silent end: tool work done, no final text (#4093).
        respond(""),
        // The wrap-up narrates intent instead of reporting the outcome.
        respond(NARRATION),
        // The check rejects it.
        respond("REJECT"),
    ]);

    let (agent, reply, streamed) =
        run_turn(recorded.clone(), vec![Box::new(FailingInstallTool)]).await;

    assert!(
        !reply.contains(NARRATION),
        "a close the check rejected must not be the reply, got: {reply}"
    );
    assert!(
        reply.contains("View it at https://example.test/demo"),
        "the fallback must carry the failing tool's own message, got: {reply}"
    );
    assert!(
        !streamed.contains(NARRATION),
        "a rejected close must never be streamed, got: {streamed}"
    );
    assert!(
        history_ends_on(&agent, &reply),
        "history must end on the reply the user saw, got: {:?}",
        agent.history.last()
    );

    let requests = recorded.requests.lock().await;
    assert_eq!(requests.len(), 4, "tool round, silent end, wrap-up, check");
    let wrap_up = &requests[2].last().expect("wrap-up instruction").content;
    assert!(
        wrap_up.contains("<tool_records>") && wrap_up.contains(INSTALL_FAILURE),
        "the wrap-up must be grounded in this turn's tool records, got: {wrap_up}"
    );
    assert_eq!(
        requests[3].len(),
        1,
        "the check must see only its own prompt, not the conversation"
    );
    assert!(
        requests[3][0].content.contains(NARRATION)
            && requests[3][0].content.contains(INSTALL_FAILURE),
        "the check must be given the candidate and the records, got: {}",
        requests[3][0].content
    );
}

/// A check that fails or answers without a verdict leaves the close unverified,
/// and unverified closing text must not ship (CodeRabbit on #6289).
#[tokio::test]
async fn an_unverified_close_is_replaced_by_the_tool_records() {
    const CANDIDATE: &str = "The demo item is installed.";
    let recorded = scripted(vec![
        respond(INSTALL_CALL),
        respond(""),
        respond(CANDIDATE),
        // No ACCEPT/REJECT verdict.
        respond("I am not sure."),
    ]);

    let (agent, reply, streamed) = run_turn(recorded, vec![Box::new(FailingInstallTool)]).await;

    assert!(
        !reply.contains(CANDIDATE),
        "a close the check could not verify must not be the reply, got: {reply}"
    );
    assert!(
        reply.contains("View it at https://example.test/demo"),
        "the fallback must carry the failing tool's own message, got: {reply}"
    );
    assert!(
        !streamed.contains(CANDIDATE),
        "an unverified close must never be streamed, got: {streamed}"
    );
    assert!(history_ends_on(&agent, &reply));
}

#[tokio::test]
async fn an_accepted_close_is_streamed_and_kept() {
    const CLOSE: &str = "I could not install the demo item: it has no direct download. \
                         You can view it at https://example.test/demo.";
    let recorded = scripted(vec![
        respond(INSTALL_CALL),
        respond(""),
        respond(CLOSE),
        respond("ACCEPT"),
    ]);

    let (agent, reply, streamed) = run_turn(recorded, vec![Box::new(FailingInstallTool)]).await;

    assert_eq!(reply, CLOSE, "an accepted close is the reply");
    assert!(
        streamed.contains(CLOSE),
        "an accepted close must be streamed once the check passes, got: {streamed}"
    );
    assert!(history_ends_on(&agent, &reply));
}

#[tokio::test]
async fn a_breaker_halt_is_closed_for_the_user_instead_of_showing_the_stop_note() {
    const CLOSE: &str = "I could not install the demo item: it has no direct download. \
                         You can view it at https://example.test/demo.";
    let recorded = scripted(vec![
        // The same failing call three times trips the identical-retry breaker.
        respond(INSTALL_CALL),
        respond(INSTALL_CALL),
        respond(INSTALL_CALL),
        // Wrap-up, then its check.
        respond(CLOSE),
        respond("ACCEPT"),
    ]);

    let (agent, reply, _) = run_turn(recorded.clone(), vec![Box::new(FailingInstallTool)]).await;

    assert!(
        !reply.contains("instead of retrying") && !reply.starts_with("Stopping:"),
        "the breaker's model-directed stop note must not be the user's reply, got: {reply}"
    );
    assert_eq!(reply, CLOSE, "the checked close is the reply");
    assert!(
        history_ends_on(&agent, &reply),
        "a halted turn's reply must be recorded in history, got: {:?}",
        agent.history.last()
    );

    let requests = recorded.requests.lock().await;
    assert_eq!(
        requests.len(),
        5,
        "three tool rounds, the wrap-up and the check; the halted loop makes no further call"
    );
    let wrap_up = &requests[3].last().expect("wrap-up instruction").content;
    assert!(
        wrap_up.contains("<stop_note>") && wrap_up.contains(INSTALL_FAILURE),
        "the wrap-up must receive the stop note and the records as input, got: {wrap_up}"
    );
}

/// XML-dialect calls carry no provider id, so both rounds' calls are `call_0`.
/// Each round must still be recorded with its own result: the second round's
/// failure is why the request was not done, and a first-match lookup replaced it
/// with the first round's success (Codex review on #6289).
#[tokio::test]
async fn each_tool_round_is_recorded_with_its_own_result() {
    let recorded = scripted(vec![
        respond(INSTALL_CALL),
        respond(INSTALL_CALL),
        respond(""),
        respond("I'll look into the demo item."),
        respond("REJECT"),
    ]);

    let (_, reply, _) = run_turn(
        recorded.clone(),
        vec![Box::new(TwoRoundInstallTool {
            calls: AtomicUsize::new(0),
        })],
    )
    .await;

    assert!(
        reply.contains("round one: found the demo item in the catalog"),
        "the first round keeps its own result, got: {reply}"
    );
    assert!(
        reply.contains("View it at https://example.test/demo"),
        "the second round must be recorded with its own failure, not the first round's result, got: {reply}"
    );
    let requests = recorded.requests.lock().await;
    let wrap_up = &requests[3].last().expect("wrap-up instruction").content;
    assert!(
        wrap_up.contains(INSTALL_FAILURE),
        "the wrap-up records must carry the second round's failure, got: {wrap_up}"
    );
}
