//! Tests for the chat-workload resolver wired into channel runtime startup.
//!
//! Issue #3098 sub-issue 1: prior to this fix, channel runtime startup
//! always built a cloud-only provider chain and used
//! `config.default_model`, ignoring the per-workload `chat_provider`
//! routing string. These tests pin the resolver behavior so the default
//! (cloud) path is preserved for users who haven't picked a local /
//! BYOK model, and the override path activates for those who have.

use super::{resolve_chat_workload, ChatWorkloadResolution, RelayInboundMessageHandler};
use crate::config::Config;
use tinychannels::relay::RelayInboundHandler;

fn config_with_chat_provider(s: Option<&str>) -> Config {
    let mut config = Config::default();
    config.chat_provider = s.map(str::to_string);
    config
}

#[test]
fn chat_provider_unset_blank_or_sentinel_resolves_to_cloud() {
    for value in [None, Some(""), Some("cloud"), Some("openhuman")] {
        let config = config_with_chat_provider(value);
        assert!(
            matches!(
                resolve_chat_workload(&config),
                ChatWorkloadResolution::Cloud
            ),
            "{value:?} must resolve to cloud"
        );
    }
}

#[test]
fn chat_provider_local_and_byok_strings_resolve_to_workload() {
    // (configured string, expected slug). The bare `claude_agent_sdk`
    // sentinel has no colon, so its slug is the full string.
    let cases = [
        ("ollama:llama3.2", "ollama"),
        ("lmstudio:qwen2.5:0.5b", "lmstudio"),
        ("openai:gpt-4o", "openai"),
        ("claude_agent_sdk", "claude_agent_sdk"),
    ];
    for (configured, expected_slug) in cases {
        let config = config_with_chat_provider(Some(configured));
        match resolve_chat_workload(&config) {
            ChatWorkloadResolution::Workload {
                provider_string,
                slug,
            } => {
                assert_eq!(provider_string, configured);
                assert_eq!(slug, expected_slug);
            }
            ChatWorkloadResolution::Cloud => {
                panic!("expected Workload for {configured}, got Cloud")
            }
        }
    }
}

#[tokio::test]
async fn relay_inbound_handler_forwards_envelopes_to_dispatch_bus() {
    let (tx, mut rx) = tokio::sync::mpsc::channel(1);
    let handler = RelayInboundMessageHandler::new(tx);
    let envelope = tinychannels::ChannelInboundEnvelope {
        channel: tinychannels::channel::ChannelRef {
            id: "telegram".into(),
            account_id: None,
        },
        message_id: "relay-msg-1".into(),
        conversation: tinychannels::channel::ConversationRef {
            id: "chat-1".into(),
            topic_id: Some("topic-1".into()),
            ..Default::default()
        },
        sender: tinychannels::channel::SenderRef {
            id: "alice".into(),
            ..Default::default()
        },
        text: "hello from relay".into(),
        ..Default::default()
    };

    handler
        .handle(tinychannels::relay::AuthenticatedRelayInboundEvent {
            event: serde_json::to_value(envelope).expect("relay envelope json"),
            buffer_id: Some("buffer-1".into()),
            delivered_via_authenticated_relay: true,
        })
        .await
        .expect("handle relay inbound");

    let runtime_msg = rx.recv().await.expect("forwarded channel message");
    let forwarded = runtime_msg.message;
    assert_eq!(forwarded.message_id, "relay-msg-1");
    assert_eq!(forwarded.channel.id, "telegram");
    assert_eq!(forwarded.conversation.id, "chat-1");
    assert_eq!(forwarded.sender.id, "alice");
    assert_eq!(forwarded.text, "hello from relay");
    assert_eq!(forwarded.conversation.thread_id.as_deref(), Some("topic-1"));
    assert_eq!(forwarded.conversation.topic_id.as_deref(), Some("topic-1"));
}

#[tokio::test]
async fn relay_inbound_handler_rejects_malformed_envelopes() {
    let (tx, _rx) = tokio::sync::mpsc::channel(1);
    let handler = RelayInboundMessageHandler::new(tx);

    let error = handler
        .handle(tinychannels::relay::AuthenticatedRelayInboundEvent {
            event: serde_json::json!("not an envelope"),
            buffer_id: None,
            delivered_via_authenticated_relay: true,
        })
        .await
        .expect_err("malformed relay payload should fail");

    assert!(error.to_string().contains("invalid inbound envelope"));
}
