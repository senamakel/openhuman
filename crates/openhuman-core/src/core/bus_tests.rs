use super::*;

#[test]
fn the_catalog_address_constants_are_valid() {
    // These are `expect`ed at runtime, so a typo must fail here rather than
    // at startup in a user's process.
    let _ = config();
    let _ = manifest();
}

#[test]
fn the_manifest_declares_the_catalog_in_both_directions() {
    let manifest = manifest();
    let interface = EVENTS_INTERFACE.try_into().unwrap();
    assert!(
        manifest.provided(&interface).is_some(),
        "openhuman publishes"
    );
    assert!(
        manifest.consumed(&interface).is_some(),
        "openhuman subscribes"
    );
}

#[tokio::test]
async fn isolated_bus_delivers() {
    use std::sync::Arc;
    use tinybus::EventHandler;
    use tokio::sync::Mutex;

    struct Capture(Arc<Mutex<Vec<DomainEvent>>>);

    #[async_trait::async_trait]
    impl EventHandler<DomainEvent> for Capture {
        fn name(&self) -> &str {
            "test::capture"
        }
        async fn handle(&self, event: &DomainEvent) {
            self.0.lock().await.push(event.clone());
        }
    }

    let bus = crate::core::bus_testing::isolated_bus().await;

    let seen = Arc::new(Mutex::new(Vec::new()));
    let _handle = bus.subscribe(Arc::new(Capture(seen.clone())));
    bus.publish(DomainEvent::SystemStartup {
        component: "test".into(),
    });

    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    while seen.lock().await.is_empty() {
        assert!(tokio::time::Instant::now() < deadline, "no delivery");
        tokio::task::yield_now().await;
    }
}
