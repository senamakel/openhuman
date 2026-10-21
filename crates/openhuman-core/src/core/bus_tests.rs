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

#[test]
fn removing_cron_delivery_moves_past_the_canonical_v2_event_catalog() {
    // The canonical 2.0 catalog already retired the desktop voice event, and
    // 2.1 added inbound-event queues. Removing CronDeliveryRequested from
    // that wire contract requires a new major version beyond both.
    let manifest = manifest();
    let interface = EVENTS_INTERFACE.try_into().unwrap();
    let advertised = manifest
        .provided(&interface)
        .expect("OpenHuman advertises its event catalog");
    assert!(
        advertised.speaks.major > 2,
        "removing CronDeliveryRequested is incompatible with the canonical 2.x catalog; got {}",
        advertised.speaks
    );
}

#[test]
fn removed_cron_delivery_is_not_a_deserializable_wire_event() {
    let error = serde_json::from_value::<crate::core::events::DomainEvent>(serde_json::json!({
        "CronDeliveryRequested": {}
    }))
    .expect_err("the removed event must not remain in the published vocabulary");
    let message = error.to_string();
    assert!(
        message.contains("unknown variant") && message.contains("CronDeliveryRequested"),
        "expected an explicit removed-variant error, got {message}"
    );
}

#[test]
fn cron_completion_tenant_remains_optional_and_round_trips() {
    let event = crate::core::events::DomainEvent::CronJobCompleted {
        job_id: "job-1".into(),
        success: true,
        output: "done".into(),
        tenant: Some(crate::core::runtime::Tenant {
            profile: Some("profile-a".into()),
            agent: Some("agent-a".into()),
        }),
    };
    let wire = serde_json::to_value(event).unwrap();
    assert_eq!(wire["CronJobCompleted"]["tenant"]["profile"], "profile-a");
    assert_eq!(wire["CronJobCompleted"]["tenant"]["agent"], "agent-a");

    let legacy = serde_json::from_value::<crate::core::events::DomainEvent>(serde_json::json!({
        "CronJobCompleted": {
            "job_id": "job-legacy",
            "success": true,
            "output": "done"
        }
    }))
    .unwrap();
    assert!(matches!(
        legacy,
        crate::core::events::DomainEvent::CronJobCompleted { tenant: None, .. }
    ));
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
