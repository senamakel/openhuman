use super::*;
use std::sync::Arc;

fn sink() -> RelayChannel {
    RelayChannel::new(
        "telegram",
        "gw-1",
        "channel:telegram/42/-100",
        "req-1",
        "m-1",
    )
}

#[test]
fn the_outbound_event_names_the_chat_and_carries_the_text() {
    let event = sink().outbound_event(&SendMessage::new("hi there", "-100"));
    assert_eq!(event.event, CHANNEL_OUTBOUND_EVENT);
    assert_eq!(event.client_id, "gw-1");
    assert_eq!(event.thread_id, "channel:telegram/42/-100");
    assert_eq!(event.request_id, "req-1");
    assert_eq!(event.full_response.as_deref(), Some("hi there"));
    let structured = event.structured.expect("structured");
    assert_eq!(structured["kind"], CHANNEL_OUTBOUND_EVENT);
    assert_eq!(structured["channel"], "telegram");
    assert_eq!(structured["chat_id"], "-100");
    assert_eq!(structured["reply_to_message_id"], "m-1");
    assert!(event.agent.is_none(), "stamped at publish, not here");
    assert!(event.profile.is_none(), "stamped at publish, not here");

    let wire = serde_json::to_value(sink().outbound_event(&SendMessage::new("x", "-100")))
        .expect("serialize");
    assert!(
        wire.get("agent").is_none(),
        "the routing stamp is not on the wire"
    );
}

#[tokio::test]
async fn send_publishes_under_the_callers_stamp_and_records_the_text() {
    let mut events = crate::web_chat::subscribe_web_channel_events();
    let context =
        crate::core::runtime::CoreContext::for_test(crate::core::runtime::DomainSet::full(), None)
            .derive_with(
                crate::core::runtime::ContextOverlay::new(
                    crate::config::Config::default(),
                    crate::core::runtime::DomainSet::full(),
                    Default::default(),
                )
                .session_agent("u-relay-test")
                .profile("p-relay-test"),
            );
    let channel = Arc::new(RelayChannel::new(
        "telegram",
        "gw-stamp",
        "channel:telegram/1/2",
        "req-stamp",
        "m-9",
    ));
    let sender = Arc::clone(&channel);
    crate::core::runtime::CoreContext::scope(context, async move {
        sender
            .send(&SendMessage::new("reply one", "2"))
            .await
            .expect("send");
    })
    .await;

    let event = loop {
        let event = events.recv().await.expect("event");
        if event.request_id == "req-stamp" {
            break event;
        }
    };
    assert_eq!(event.event, CHANNEL_OUTBOUND_EVENT);
    assert_eq!(event.agent.as_deref(), Some("u-relay-test"));
    assert!(event.belongs_to("u-relay-test"));
    assert!(!event.belongs_to("u-someone-else"));
    assert_eq!(event.profile.as_deref(), Some("p-relay-test"));
    assert!(event.belongs_to_profile("p-relay-test"));
    assert!(!event.belongs_to_profile("p-someone-else"));
    assert_eq!(channel.sent(), vec!["reply one".to_string()]);
}

#[tokio::test]
async fn a_relay_has_no_listener() {
    let (tx, _rx) = tokio::sync::mpsc::channel(1);
    assert!(sink().listen(tx).await.is_err());
    assert_eq!(sink().name(), "telegram");
    assert!(!sink().supports_draft_updates());
}
