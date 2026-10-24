use super::*;
use tinychannels_bus::{ChannelInboundEnvelope, ChannelRef, ConversationRef, SenderRef};

fn queued(seq: u64, id: &str, timestamp: u64, content: &str) -> QueuedInbound {
    QueuedInbound {
        seq,
        inbound: AdmittedInbound {
            msg: ChannelInboundEnvelope {
                message_id: id.to_string(),
                sender: SenderRef {
                    id: "alice".to_string(),
                    ..Default::default()
                },
                conversation: ConversationRef {
                    id: format!("reply-{id}"),
                    thread_id: Some(format!("thread-{id}")),
                    ..Default::default()
                },
                text: content.to_string(),
                channel: ChannelRef {
                    id: "telegram".to_string(),
                    ..Default::default()
                },
                timestamp_unix_ms: Some(timestamp),
                ..Default::default()
            },
            history_key: "telegram_alice_reply".to_string(),
        },
    }
}

#[test]
fn single_message_passes_through_unchanged() {
    let one = queued(1, "m1", 5, "  hello  ");
    let merged = merge_channel_batch(vec![one.clone()]).unwrap();
    assert_eq!(merged.msg.text, "  hello  ");
    assert_eq!(merged.msg.message_id, "m1");
    assert_eq!(
        merged.msg.conversation.thread_id.as_deref(),
        Some("thread-m1")
    );
    assert_eq!(merged.history_key, one.inbound.history_key);
    assert!(merge_channel_batch(Vec::new()).is_none());
}

#[test]
fn merge_orders_by_timestamp_then_receive_order_and_uses_newest_as_base() {
    let batch = vec![
        queued(3, "m3", 20, "third"),
        queued(2, "m2", 10, "second"),
        queued(1, "m1", 10, "first"),
    ];
    let merged = merge_channel_batch(batch).unwrap();
    assert_eq!(merged.msg.text, "first\nsecond\nthird");
    assert_eq!(merged.msg.message_id, "m3");
    assert_eq!(merged.msg.conversation.id, "reply-m3");
    assert_eq!(
        merged.msg.conversation.thread_id.as_deref(),
        Some("thread-m3")
    );
    assert_eq!(merged.msg.timestamp_unix_ms, Some(20));
}

#[test]
fn image_caption_then_text_keeps_marker_in_place_and_skips_blanks() {
    let batch = vec![
        queued(1, "m1", 1, "[IMAGE:/tmp/a.png]\nlook at this"),
        queued(2, "m2", 2, "   "),
        queued(3, "m3", 3, "what is it?"),
    ];
    let merged = merge_channel_batch(batch).unwrap();
    assert_eq!(
        merged.msg.text,
        "[IMAGE:/tmp/a.png]\nlook at this\nwhat is it?"
    );
}

#[test]
fn sender_name_is_the_last_one_present() {
    let mut a = queued(1, "m1", 1, "a");
    a.inbound.msg.sender.name = Some("Alice A".into());
    let mut b = queued(2, "m2", 2, "b");
    b.inbound.msg.sender.name = Some("Alice B".into());
    let c = queued(3, "m3", 3, "c");
    let merged = merge_channel_batch(vec![a, b, c]).unwrap();
    assert_eq!(merged.msg.sender.name.as_deref(), Some("Alice B"));
}

#[test]
fn item_traits_classify_media_commands_and_blanks() {
    let image = queued(1, "m1", 1, "[IMAGE:x] caption");
    assert!(image.carries_media());
    assert!(image.mergeable());
    assert_eq!(image.chars(), "[IMAGE:x] caption".chars().count());
    assert!(queued(1, "m", 1, "[FILE:doc.pdf]").carries_media());
    assert!(!queued(1, "m", 1, "plain ✓").carries_media());
    assert_eq!(queued(1, "m", 1, "plain ✓").chars(), 7);
    assert!(!queued(1, "m", 1, "  /status").mergeable());
    assert!(!queued(1, "m", 1, " \n ").mergeable());
}
