use super::*;

struct EveryBusPeerIsAlpha;

#[async_trait]
impl CallerDirectory for EveryBusPeerIsAlpha {
    async fn module_for(&self, sender: &BusName) -> Option<String> {
        sender.is_unique().then(|| "alpha".to_owned())
    }
}

#[tokio::test]
async fn detach_monitor_does_not_keep_its_serving_connection_alive() {
    use tinybus::broker::Broker;
    use tinybus::transport::memory::MemoryBus;
    use tinybus::Connection;

    const CLOSE_DEADLINE: std::time::Duration = std::time::Duration::from_secs(5);

    let bus = MemoryBus::new();
    Broker::new().spawn(bus.clone());
    let observer = Connection::connect(bus.connect().await.unwrap())
        .await
        .unwrap();
    let service = Connection::connect(bus.connect().await.unwrap())
        .await
        .unwrap();
    let service_name = service.unique_name().unwrap();
    let rig = rig();
    let host = Arc::new(StorageHost::new(
        Arc::new(EveryBusPeerIsAlpha),
        Arc::clone(&rig.env) as Arc<dyn Environment>,
        Arc::clone(&rig.trail) as Arc<dyn AuditSink>,
    ));
    let served = serve_objects(&service, &host, None).await.unwrap();

    drop(served);
    drop(service);
    tokio::time::timeout(CLOSE_DEADLINE, async {
        loop {
            if observer
                .name_owner(service_name.as_str())
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
    .expect("dropping the final service handle closes its broker peer");
}

#[tokio::test]
async fn a_queued_watch_open_cannot_restore_a_detached_peers_relay() {
    use tinybus::broker::Broker;
    use tinybus::transport::memory::MemoryBus;
    use tinybus::Connection;
    use tinystoragedrivers_busproxy::{StorageCall, StorageEndpoint};

    const DETACH_DEADLINE: std::time::Duration = std::time::Duration::from_secs(5);

    let bus = MemoryBus::new();
    Broker::new().spawn(bus.clone());
    let service = Connection::connect(bus.connect().await.unwrap())
        .await
        .unwrap();
    let peer = Connection::connect(bus.connect().await.unwrap())
        .await
        .unwrap();
    let sender = peer.unique_name().unwrap();
    let rig = rig();
    let host = Arc::new(StorageHost::new(
        Arc::new(EveryBusPeerIsAlpha),
        Arc::clone(&rig.env) as Arc<dyn Environment>,
        Arc::clone(&rig.trail) as Arc<dyn AuditSink>,
    ));
    super::super::endpoint::watch_peer_detach(&service, Arc::clone(&host))
        .await
        .unwrap();
    let endpoint =
        super::super::endpoint::HostEndpoint::new(Arc::clone(&host), service.downgrade(), None);

    // The busproxy asks synchronously for signals before dispatching WatchOpen.
    let _signals = endpoint.watch_signals(&sender).unwrap();
    drop(peer);
    tokio::time::timeout(DETACH_DEADLINE, async {
        loop {
            let detached = service.name_owner(sender.as_str()).await.unwrap().is_none();
            let released = !host.relays.lock().unwrap().contains_key(&sender);
            if detached && released {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the broker's detach event releases the first relay");

    // A MethodCall task is spawned separately from the signal reader. Model a
    // queued WatchOpen whose synchronous signal request runs after detach.
    let _late_signals = endpoint.watch_signals(&sender).unwrap();
    assert!(host.relays.lock().unwrap().contains_key(&sender));
    endpoint
        .call(StorageCall {
            object: Object::Documents,
            selector: "WatchOpen",
            request: json!({"collection":"notes","filter":{"op":"all"},"interval_ms":20}),
            sender: Some(&sender),
            admission: Admission::Private,
        })
        .await
        .unwrap();
    assert!(
        !host.relays.lock().unwrap().contains_key(&sender),
        "the post-call liveness check removes a relay recreated after detach"
    );
}

/// Exercises the storage endpoint through a real in-memory TinyBus broker.
/// Each sender opens a watch and detaches before the next sender is created.
/// The first watch stays active after detach; the rest are closed first. A live
/// anchor watch stays active through the churn.
#[tokio::test]
async fn detached_peer_watch_relays_are_released_before_the_256_relay_limit() {
    use tinybus::broker::Broker;
    use tinybus::router::MatchRule;
    use tinybus::transport::memory::MemoryBus;
    use tinybus::{Connection, InterfaceName, MemberName};
    use tinystoragedrivers_bus::STORAGE_INTERFACE;
    use tinystoragedrivers_busproxy::{watch_path, OBJECT_ROOT, WATCH_MEMBER};

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

    let anchor = Connection::connect(bus.connect().await.unwrap())
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

    let mut previous_name: Option<BusName> = None;
    let mut last_peer: Option<Connection> = None;
    let mut last_signals = None;
    let mut last_watch_id = None;
    for index in 1..=SENDERS {
        if let Some(previous) = previous_name.take() {
            tokio::time::timeout(DETACH_DEADLINE, async {
                loop {
                    let detached = service
                        .name_owner(previous.as_str())
                        .await
                        .unwrap()
                        .is_none();
                    let relay_released = !host.relays.lock().unwrap().contains_key(&previous);
                    if detached && relay_released {
                        break;
                    }
                    tokio::task::yield_now().await;
                }
            })
            .await
            .expect("broker detaches the previous sender and releases its relay");
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
            previous_name = Some(unique);
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
    let final_watch_id = last_watch_id.expect("the 300th sender opened a watch");
    let final_signal = tokio::time::timeout(DETACH_DEADLINE, async {
        loop {
            let signal = last_signals.as_mut().unwrap().recv().await.unwrap();
            if signal.body[0]["watch_id"] == final_watch_id {
                break signal;
            }
        }
    })
    .await
    .expect("the final caller's watch signal arrives");
    assert_eq!(final_signal.body[0]["watch_id"], final_watch_id);

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
