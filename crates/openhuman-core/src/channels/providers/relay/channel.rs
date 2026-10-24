//! `RelayChannel`: the reply sink of a relayed turn.
//!
//! The core never talks to the platform here. Each reply the dispatch
//! pipeline sends becomes a [`CHANNEL_OUTBOUND_EVENT`] on the web-channel
//! event bus, which reaches the caller's own `/events` stream; the gateway
//! that relayed the message delivers it to the platform.

use std::sync::Mutex;

use async_trait::async_trait;
use serde_json::json;

use crate::web_chat::{publish_web_channel_event, WebChannelEvent};
use tinychannels_bus::channel::{ContentPart, OutboundContentPart};
use tinychannels_bus::{Channel, ChannelOutboundMessage};

/// The event name a relayed reply is published under.
pub const CHANNEL_OUTBOUND_EVENT: &str = "channel_outbound";

/// The reply sink for one relayed message.
pub struct RelayChannel {
    /// The platform (`telegram`): the name the pipeline looks the sink up by.
    channel: String,
    /// The `/events` client id the gateway listens on.
    client_id: String,
    /// The conversation's thread.
    thread_id: String,
    /// Correlates the replies with the `channel_relay_inbound` call.
    request_id: String,
    /// The platform message the replies answer.
    reply_to: String,
    /// Every text sent, in order; persisted as the turn's reply.
    sent: Mutex<Vec<String>>,
}

impl RelayChannel {
    pub fn new(
        channel: impl Into<String>,
        client_id: impl Into<String>,
        thread_id: impl Into<String>,
        request_id: impl Into<String>,
        reply_to: impl Into<String>,
    ) -> Self {
        Self {
            channel: channel.into(),
            client_id: client_id.into(),
            thread_id: thread_id.into(),
            request_id: request_id.into(),
            reply_to: reply_to.into(),
            sent: Mutex::new(Vec::new()),
        }
    }

    /// The texts sent so far, oldest first.
    pub fn sent(&self) -> Vec<String> {
        self.sent.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// The event that carries `message` to the gateway. The routing stamps
    /// (`agent`, `profile`) are left for `publish_web_channel_event`, which
    /// takes them from the publishing context's tenant, so `/events` hands the
    /// reply only to the caller's own stream.
    pub fn outbound_event(
        &self,
        message: &ChannelOutboundMessage,
    ) -> anyhow::Result<WebChannelEvent> {
        let text = outbound_text(message)?;
        Ok(WebChannelEvent {
            event: CHANNEL_OUTBOUND_EVENT.to_string(),
            client_id: self.client_id.clone(),
            thread_id: self.thread_id.clone(),
            request_id: self.request_id.clone(),
            full_response: Some(text),
            structured: Some(json!({
                "kind": CHANNEL_OUTBOUND_EVENT,
                "channel": self.channel,
                "chat_id": message.conversation_id,
                "thread_ts": message.thread_id,
                "reply_to_message_id": self.reply_to,
                "idempotency_key": message.idempotency_key,
            })),
            ..Default::default()
        })
    }
}

#[async_trait]
impl Channel for RelayChannel {
    fn name(&self) -> &str {
        &self.channel
    }

    #[allow(
        deprecated,
        reason = "TinyChannels v0.1.39 requires the v21 Channel::send bridge; host sends use send_typed"
    )]
    async fn send(&self, message: &tinychannels_bus::SendMessage) -> anyhow::Result<()> {
        let typed = ChannelOutboundMessage {
            channel_id: self.channel.clone(),
            conversation_id: message.recipient.clone(),
            channel_ref: None,
            conversation_ref: Some(tinychannels_bus::ConversationRef {
                id: message.recipient.clone(),
                thread_id: message.thread_ts.clone(),
                topic_id: message.topic_id.clone(),
                ..Default::default()
            }),
            reply_to_id: message.reply_to_id.clone(),
            thread_id: message.thread_ts.clone(),
            turn_id: None,
            idempotency_key: message.idempotency_key.clone().unwrap_or_default(),
            parts: vec![OutboundContentPart::Content {
                part: ContentPart::Text {
                    text: message.content.clone(),
                    markdown_dialect: None,
                },
            }],
        };
        self.send_typed(&typed).await
    }

    async fn send_typed(&self, message: &ChannelOutboundMessage) -> anyhow::Result<()> {
        let event = self.outbound_event(message)?;
        let content = outbound_text(message)?;
        tracing::debug!(
            channel = %self.channel,
            thread_id = %self.thread_id,
            request_id = %self.request_id,
            chars = content.chars().count(),
            "[channels::relay] publishing channel_outbound"
        );
        self.sent
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(content);
        publish_web_channel_event(event);
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
        anyhow::bail!(
            "a relayed channel has no listener: its messages arrive through channel_relay_inbound"
        )
    }
}

fn outbound_text(message: &ChannelOutboundMessage) -> anyhow::Result<String> {
    let mut text = String::new();
    for part in &message.parts {
        match part {
            OutboundContentPart::Content {
                part: ContentPart::Text { text: segment, .. },
            } => text.push_str(segment),
            _ => anyhow::bail!("channel relay supports text outbound content only"),
        }
    }
    Ok(text)
}

#[cfg(test)]
#[path = "channel_tests.rs"]
mod tests;
