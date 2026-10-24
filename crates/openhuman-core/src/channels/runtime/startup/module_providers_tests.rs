use super::outbound_intent;
use tinychannels_bus::channel::{ContentPart, OutboundContentPart};
use tinychannels_bus::{ChannelOutboundMessage, ChannelRef, ConversationRef, OutboundPayload};

#[test]
fn outbound_channel_message_becomes_an_equivalent_intent() {
    let message = ChannelOutboundMessage {
        channel_id: "telegram".into(),
        conversation_id: "chat-1".into(),
        channel_ref: Some(ChannelRef {
            id: "telegram-account".into(),
            account_id: Some("account-1".into()),
        }),
        conversation_ref: Some(ConversationRef {
            id: "chat-1".into(),
            thread_id: Some("thread-1".into()),
            topic_id: Some("topic-1".into()),
            ..Default::default()
        }),
        reply_to_id: Some("source-1".into()),
        thread_id: Some("thread-1".into()),
        turn_id: Some("turn-1".into()),
        idempotency_key: "turn-1:segment-0".into(),
        parts: vec![OutboundContentPart::Content {
            part: ContentPart::Text {
                text: "hello".into(),
                markdown_dialect: None,
            },
        }],
    };

    let intent = outbound_intent(&message);
    assert_eq!(intent.channel_id, message.channel_id);
    assert_eq!(intent.conversation_id, message.conversation_id);
    assert_eq!(intent.channel_ref, message.channel_ref);
    assert_eq!(intent.conversation_ref, message.conversation_ref);
    assert_eq!(intent.reply_to_id, message.reply_to_id);
    assert_eq!(intent.thread_id, message.thread_id);
    assert_eq!(intent.turn_id, message.turn_id);
    assert_eq!(intent.idempotency_key, message.idempotency_key);
    assert!(matches!(
        intent.payload,
        OutboundPayload::Parts { parts } if parts == message.parts
    ));
}
