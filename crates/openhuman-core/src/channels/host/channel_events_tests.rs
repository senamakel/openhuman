//! Tests for the channel approval-surface and turn-state subscribers.

use super::*;
use std::path::Path;
use std::sync::Mutex as StdMutex;
#[cfg(feature = "e2e-test-support")]
use tempfile::tempdir;
use tinychannels_bus::ChannelOutboundMessage;

struct RecordingChannel {
    name: String,
    sent: StdMutex<Vec<ChannelOutboundMessage>>,
}

#[async_trait]
impl Channel for RecordingChannel {
    fn name(&self) -> &str {
        &self.name
    }
    #[allow(
        deprecated,
        reason = "TinyChannels v0.1.39 requires the v21 Channel::send method on test doubles"
    )]
    async fn send(&self, _message: &tinychannels_bus::SendMessage) -> anyhow::Result<()> {
        anyhow::bail!("tests exercise the typed v22 send method")
    }
    async fn send_typed(&self, message: &ChannelOutboundMessage) -> anyhow::Result<()> {
        self.sent.lock().unwrap().push(message.clone());
        Ok(())
    }
    #[allow(
        deprecated,
        reason = "TinyChannels v0.1.39 still requires the legacy listener method on Channel"
    )]
    async fn listen(
        &self,
        _tx: tokio::sync::mpsc::Sender<tinychannels_bus::ChannelMessage>,
    ) -> anyhow::Result<()> {
        Ok(())
    }
    fn supports_chat_approvals(&self) -> bool {
        matches!(self.name.as_str(), "telegram" | "discord" | "slack")
    }
    fn supports_remote_control(&self) -> bool {
        matches!(self.name.as_str(), "telegram" | "discord")
    }
}

fn recording(name: &str) -> Arc<RecordingChannel> {
    Arc::new(RecordingChannel {
        name: name.into(),
        sent: StdMutex::new(Vec::new()),
    })
}

fn approval_subscriber(channels: &[Arc<RecordingChannel>]) -> ChannelApprovalSurfaceSubscriber {
    let map: HashMap<String, Arc<dyn Channel>> = channels
        .iter()
        .map(|c| (c.name.clone(), Arc::clone(c) as Arc<dyn Channel>))
        .collect();
    ChannelApprovalSurfaceSubscriber::new(Arc::new(map))
}

#[cfg(feature = "e2e-test-support")]
fn turn_state_subscriber(workspace_dir: PathBuf, names: &[&str]) -> ChannelTurnStateSubscriber {
    let channels: HashMap<String, Arc<dyn Channel>> = names
        .iter()
        .map(|name| {
            let channel = recording(name);
            ((*name).to_owned(), channel as Arc<dyn Channel>)
        })
        .collect();
    let mut subscriber = ChannelTurnStateSubscriber::new(workspace_dir);
    subscriber.channels = Arc::new(channels);
    subscriber
}

fn received(channel: &str, reply_target: &str, thread_ts: Option<&str>, ws: &Path) -> DomainEvent {
    DomainEvent::ChannelMessageReceived {
        channel: channel.into(),
        message_id: "m1".into(),
        sender: "alice".into(),
        reply_target: reply_target.into(),
        content: "hi".into(),
        thread_ts: thread_ts.map(str::to_string),
        inbound_envelope: None,
        workspace_dir: ws.to_path_buf(),
    }
}

#[cfg(feature = "e2e-test-support")]
fn processed(channel: &str, reply_target: &str, ws: &Path) -> DomainEvent {
    DomainEvent::ChannelMessageProcessed {
        channel: channel.into(),
        message_id: "m1".into(),
        sender: "alice".into(),
        reply_target: reply_target.into(),
        content: "hi".into(),
        thread_ts: None,
        response: "ok".into(),
        provider: "test-provider".into(),
        model: "test-model".into(),
        elapsed_ms: 10,
        success: true,
        workspace_dir: ws.to_path_buf(),
    }
}

#[cfg(feature = "e2e-test-support")]
async fn remote_status(ws: &Path, channel: &str, reply_target: &str) -> Option<String> {
    let result: tinychannels_bus::module_remote::RemoteOperationResult =
        crate::channels::module_fixture::client()
            .await
            .call_confidential(
                "tinychannels",
                tinychannels_bus::methods::EXECUTE_REMOTE,
                (tinychannels_bus::module_remote::RemoteOperation::Execute {
                    workspace_dir: ws.to_string_lossy().into_owned(),
                    message: crate::channels::routes::route_compat_message(
                        &tinychannels_bus::ChannelInboundEnvelope {
                            channel: tinychannels_bus::ChannelRef {
                                id: channel.into(),
                                account_id: None,
                            },
                            message_id: "status-1".into(),
                            conversation: tinychannels_bus::ConversationRef {
                                id: reply_target.into(),
                                ..Default::default()
                            },
                            sender: tinychannels_bus::SenderRef {
                                id: "alice".into(),
                                ..Default::default()
                            },
                            text: "/status".into(),
                            ..Default::default()
                        },
                    ),
                    route: tinychannels_bus::module_remote::RemoteRoute {
                        provider: "openai".into(),
                        model: "test-model".into(),
                    },
                    history_len: 0,
                },),
            )
            .await
            .expect("the linked module should answer the status command");
    result.reply
}

fn approval(thread_id: Option<&str>, client_id: Option<&str>) -> DomainEvent {
    DomainEvent::ApprovalRequested {
        request_id: "req-1".into(),
        tool_name: "file_write".into(),
        action_summary: "Write notes/today.md (1.2 KiB)".into(),
        args_redacted: serde_json::json!({"path": "notes/today.md"}),
        thread_id: thread_id.map(str::to_string),
        client_id: client_id.map(str::to_string),
        tool_call_id: None,
        expires_at: None,
        agent_id: None,
    }
}

#[tokio::test]
async fn approval_prompts_reach_every_chat_channel() {
    let ws = PathBuf::from("/tmp");
    for (channel, thread_ts, key) in [
        ("telegram", Some("7"), "telegram_alice_chat-1"),
        ("discord", None, "discord_alice_chat-1"),
        ("slack", Some("1700.1"), "slack_alice_chat-1_thread:1700.1"),
    ] {
        let ch = recording(channel);
        let sub = approval_subscriber(&[Arc::clone(&ch)]);
        assert_eq!(sub.name(), "channels::approval_surface");
        assert_eq!(sub.domains(), Some(&["channel", "approval"][..]));
        sub.handle(&received(channel, "chat-1", thread_ts, &ws))
            .await;
        assert!(sub.replies.lock().unwrap().contains_key(key), "{channel}");
        sub.handle(&approval(Some(key), Some(channel))).await;
        let sent = ch.sent.lock().unwrap().clone();
        assert_eq!(sent.len(), 1, "{channel}");
        assert_eq!(sent[0].conversation_id, "chat-1");
        assert_eq!(sent[0].thread_id.as_deref(), thread_ts);
        assert!(matches!(
            &sent[0].parts[0],
            tinychannels_bus::channel::OutboundContentPart::Content {
                part: tinychannels_bus::channel::ContentPart::Text { text, .. }
            } if text.contains("file_write")
        ));
    }
}

#[tokio::test]
async fn approvals_for_web_email_or_unrouted_requests_are_ignored() {
    let ws = PathBuf::from("/tmp");
    let email = recording("email");
    let discord = recording("discord");
    let sub = approval_subscriber(&[Arc::clone(&email), Arc::clone(&discord)]);
    sub.handle(&received("email", "a@b.c", None, &ws)).await;
    sub.handle(&approval(Some("email_alice_a@b.c"), Some("email")))
        .await;
    sub.handle(&approval(Some("web-thread"), Some("web-client-1")))
        .await;
    sub.handle(&approval(Some("x"), None)).await;
    sub.handle(&approval(None, Some("discord"))).await;
    // No recorded context for this conversation → nothing sent.
    sub.handle(&approval(Some("discord_bob_c"), Some("discord")))
        .await;
    sub.handle(&DomainEvent::SystemStartup {
        component: "test".into(),
    })
    .await;
    assert!(email.sent.lock().unwrap().is_empty());
    assert!(discord.sent.lock().unwrap().is_empty());
}

// Busy flags live in the module's own storage scope and are written by the
// module, so this runs against the linked-module fixture
// (`channels::module_fixture`).
#[tokio::test]
#[cfg(feature = "e2e-test-support")]
async fn turn_state_marks_busy_for_every_remote_control_channel() {
    let _storage = crate::channels::module_fixture::install_memory_storage();
    let dir = tempdir().unwrap();
    let ws = dir.path().to_path_buf();
    let sub = turn_state_subscriber(ws.clone(), &["telegram", "discord"]);
    assert_eq!(sub.name(), "channels::turn_state");
    assert_eq!(sub.domains(), Some(&["channel"][..]));
    for channel in ["telegram", "discord"] {
        sub.handle(&received(channel, "chat-99", None, &ws)).await;
        let busy = remote_status(&ws, channel, "chat-99")
            .await
            .expect("/status should answer after a received event");
        assert!(busy.contains("in progress"), "{channel}: {busy}");

        sub.handle(&processed(channel, "chat-99", &ws)).await;
        let idle = remote_status(&ws, channel, "chat-99")
            .await
            .expect("/status should answer after a processed event");
        assert!(idle.contains("idle"), "{channel}: {idle}");
    }
}

#[tokio::test]
#[cfg(feature = "e2e-test-support")]
async fn turn_state_ignores_non_remote_channels_and_stale_workspaces() {
    let _storage = crate::channels::module_fixture::install_memory_storage();
    let dir = tempdir().unwrap();
    let ws = dir.path().to_path_buf();
    let sub = turn_state_subscriber(ws.clone(), &["telegram"]);
    let other = tempdir().unwrap().path().to_path_buf();
    sub.handle(&received("email", "a@b.c", None, &ws)).await;
    assert!(remote_status(&ws, "email", "a@b.c").await.is_none());

    sub.handle(&received("telegram", "chat-1", None, &ws)).await;
    let busy = remote_status(&ws, "telegram", "chat-1")
        .await
        .expect("/status should answer after a received event");
    assert!(busy.contains("in progress"), "{busy}");

    sub.handle(&processed("telegram", "chat-1", &other)).await;
    let still_busy = remote_status(&ws, "telegram", "chat-1")
        .await
        .expect("/status should answer after a stale processed event");
    assert!(
        still_busy.contains("in progress"),
        "a stale workspace must not clear turn state: {still_busy}"
    );

    sub.handle(&processed("telegram", "chat-1", &ws)).await;
    let idle = remote_status(&ws, "telegram", "chat-1")
        .await
        .expect("/status should answer after a matching processed event");
    assert!(idle.contains("idle"), "{idle}");
}
