use super::*;

struct EveryBusPeerIsAlpha;

#[async_trait]
impl CallerDirectory for EveryBusPeerIsAlpha {
    async fn module_for(&self, sender: &BusName) -> Option<String> {
        sender.is_unique().then(|| "alpha".to_owned())
    }
}

/// Exercises the storage endpoint through a real in-memory TinyBus broker.
/// Each sender opens and closes a watch, then its connection detaches before
/// the next sender is created. One anchor watch stays active through the churn.
#[tokio::test]
async fn detached_peer_watch_relays_are_released_before_the_256_relay_limit() {
    use tinybus::broker::Broker;
    use tinybus::router::MatchRule;
    use tinybus::transport::memory::MemoryBus;
    use tinybus::{Connection, InterfaceName, MemberName};
    use tinystoragedrivers_bus::STORAGE_INTERFACE;
    use tinystoragedrivers_busproxy::{OBJECT_ROOT, WATCH_MEMBER, watch_path};

    const SENDERS: usize = 300;
    const DETACH_DEADLINE: std::time::Duration = std::time::Duration::from_secs(5);

    let bus = MemoryBus::new();
    Broker::new().spawn(bus.clone());
    let service = Connection::connect(bus.connect().await.unwrap())
        .await
        .unwrap();
    let rig = rig();
    let host = Arc::new(StorageHost::new(
        Arc::new(EveryBusPeerIsAlpha),
        Arc::clone(&rig.env) as Arc<dyn Environment>,
        Arc::clone(&rig.trail) as Arc<dyn AuditSink>,
    ));
    let _served = serve_objects(&service, &host, None).await.unwrap();

    let mut anchor = Connection::connect(bus.connect().await.unwrap())
        .await
        .unwrap();
    anchor
        .request_name("ai.tinyhumans.test.StorageAnchor")
        .await
        .unwrap();
    let anchor_name = anchor.unique_name().unwrap();
    let mut anchor_rule = MatchRule::new()
        .signals()
        .interface(InterfaceName::new(STORAGE_INTERFACE).unwrap())
        .member(MemberName::new(WATCH_MEMBER).unwrap());
    anchor_rule.path = Some(watch_path(&anchor_name).unwrap());
    let mut anchor_signals = anchor.add_match(anchor_rule).await.unwrap();
    let documents = |connection: &Connection| {
        connection
            .proxy(
                STORAGE_INTERFACE,
                format!("{OBJECT_ROOT}/Documents"),
                STORAGE_INTERFACE,
            )
            .unwrap()
    };
    let anchor_documents = documents(&anchor);
    let anchor_watch: Value = anchor_documents
        .call(
            "WatchOpen",
            json!({"collection":"notes","filter":{"op":"all"},"interval_ms":20}),
        )
        .await
        .unwrap();
    let anchor_watch_id = anchor_watch["watch_id"].as_str().unwrap().to_owned();
    anchor
        .release_name("ai.tinyhumans.test.StorageAnchor")
        .await
        .unwrap();

    let mut previous_name: Option<String> = None;
    let mut last_peer: Option<Connection> = None;
    let mut last_signals = None;
    let mut last_watch_id = None;
    for index in 1..=SENDERS {
        if let Some(previous) = previous_name.take() {
            tokio::time::timeout(DETACH_DEADLINE, async {
                loop {
                    if service
                        .name_owner(previous.as_str())
                        .await
                        .unwrap()
                        .is_none()
                    {
                        break;
                    }
                    tokio::task::yield_now().await;
                }
            })
            .await
            .expect("broker observes the previous sender detaching");
        }

        let peer = Connection::connect(bus.connect().await.unwrap())
            .await
            .unwrap();
        let unique = peer.unique_name().unwrap();
        let mut signals = if index == SENDERS {
            let mut rule = MatchRule::new()
                .signals()
                .interface(InterfaceName::new(STORAGE_INTERFACE).unwrap())
                .member(MemberName::new(WATCH_MEMBER).unwrap());
            rule.path = Some(watch_path(&unique).unwrap());
            Some(peer.add_match(rule).await.unwrap())
        } else {
            None
        };
        let documents = documents(&peer);
        let opened: Value = documents
            .call(
                "WatchOpen",
                json!({"collection":"notes","filter":{"op":"all"},"interval_ms":20}),
            )
            .await
            .unwrap();
        let watch_id = opened["watch_id"].as_str().unwrap().to_owned();
        if index < SENDERS {
            // Leave one dead peer's watch active to prove detach tears down
            // its relay without consuming the shared server's watch budget.
            if index > 1 {
                let _: Value = documents
                    .call("WatchClose", json!({"watch_id":watch_id}))
                    .await
                    .unwrap();
            }
            drop(documents);
            drop(peer);
            previous_name = Some(unique.as_str().to_owned());
        } else {
            last_peer = Some(peer);
            last_signals = signals.take();
            last_watch_id = Some(watch_id);
        }
    }

    let final_peer = last_peer.expect("the 300th sender remains connected");
    let final_documents = documents(&final_peer);
    let _: Value = final_documents
        .call("Put", put("notes", "after-churn", json!({"v": 300})))
        .await
        .unwrap();
    let final_signal = tokio::time::timeout(DETACH_DEADLINE, last_signals.as_mut().unwrap().recv())
        .await
        .expect("the final caller's watch signal arrives")
        .unwrap();
    assert_eq!(
        final_signal.body[0]["watch_id"],
        last_watch_id.expect("the 300th sender opened a watch")
    );

    // The anchor is still live and receives its own invalidation after churn.
    let _: Value = anchor_documents
        .call("Put", put("notes", "anchor-after-churn", json!({"v": 1})))
        .await
        .unwrap();
    let anchor_signal = tokio::time::timeout(DETACH_DEADLINE, anchor_signals.recv())
        .await
        .expect("the anchor watch remains live")
        .unwrap();
    assert_eq!(anchor_signal.body[0]["watch_id"], anchor_watch_id);
}
