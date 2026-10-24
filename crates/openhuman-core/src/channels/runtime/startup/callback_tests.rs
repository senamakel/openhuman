//! Trust checks on the module-facing inbound callback: a module instance may
//! only deliver for its own channel, and an oversize payload is refused before
//! it reaches dispatch.

use super::ChannelHostCallbacks;
use crate::channels::runtime::dispatch::RuntimeChannelMessage;
use crate::config::Config;
use std::sync::{Arc, OnceLock};
use tokio::sync::mpsc;

fn callbacks() -> (ChannelHostCallbacks, mpsc::Receiver<RuntimeChannelMessage>) {
    let (tx, rx) = mpsc::channel(1);
    let callbacks = ChannelHostCallbacks::new(
        &crate::core::runtime::DispatchCtx::ambient(),
        Arc::new(Config::default()),
        Arc::new(OnceLock::new()),
        tx,
    );
    (callbacks, rx)
}

fn envelope(channel: &str, text: &str) -> tinychannels::ChannelInboundEnvelope {
    tinychannels::ChannelInboundEnvelope {
        channel: tinychannels::channel::ChannelRef {
            id: channel.into(),
            account_id: None,
        },
        message_id: "m-1".into(),
        conversation: tinychannels::channel::ConversationRef {
            id: "chat-1".into(),
            ..Default::default()
        },
        sender: tinychannels::channel::SenderRef {
            id: "alice".into(),
            ..Default::default()
        },
        text: text.into(),
        ..Default::default()
    }
}

#[tokio::test]
async fn a_message_for_the_callback_channel_is_dispatched() {
    let (callbacks, mut rx) = callbacks();
    let message = serde_json::to_value(envelope("telegram", "hi")).unwrap();
    callbacks
        .deliver_inbound("telegram".into(), message)
        .await
        .expect("own channel");
    let forwarded = rx.try_recv().expect("dispatched");
    assert_eq!(forwarded.message.channel.id, "telegram");
    assert!(!forwarded.authenticated_relay);
}

#[tokio::test]
async fn a_message_for_another_channel_is_refused() {
    let (callbacks, mut rx) = callbacks();
    let message = serde_json::to_value(envelope("discord", "hi")).unwrap();
    let error = callbacks
        .deliver_inbound("telegram".into(), message)
        .await
        .expect_err("another channel's message");
    assert!(error.to_string().contains("does not match"), "{error}");
    assert!(rx.try_recv().is_err(), "nothing may reach dispatch");
}

#[tokio::test]
async fn a_relay_envelope_for_another_channel_is_refused() {
    let (callbacks, mut rx) = callbacks();
    let delivery = serde_json::json!({
        "envelope": envelope("discord", "hi"),
        "delivered_via_authenticated_relay": true,
    });
    let error = callbacks
        .deliver_inbound("telegram".into(), delivery)
        .await
        .expect_err("another channel's relay envelope");
    assert!(error.to_string().contains("does not match"), "{error}");
    assert!(rx.try_recv().is_err(), "nothing may reach dispatch");
}

#[tokio::test]
async fn an_oversize_payload_is_refused() {
    let (callbacks, mut rx) = callbacks();
    let big = "x".repeat(super::MAX_CALLBACK_BYTES + 1);
    let message = serde_json::to_value(envelope("telegram", &big)).unwrap();
    let error = callbacks
        .deliver_inbound("telegram".into(), message)
        .await
        .expect_err("oversize payload");
    assert!(error.to_string().contains("exceeds its limit"), "{error}");
    assert!(rx.try_recv().is_err(), "nothing may reach dispatch");
}
