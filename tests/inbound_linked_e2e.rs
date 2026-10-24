//! The inbound event queues end to end, with the `tinyeventqueues` module
//! served from the linked executable.
//!
//! Local only: it needs `--features linked-modules,inbound-queues`, which no CI
//! lane or product build enables (the CI-run regression tests drive the same
//! module linked into the core's unit tests). The module is admitted through
//! the process-wide module runtime exactly as `[modules] source = "linked"`
//! does: the host's storage backs its queues (an in-memory backend here), the
//! host's `Deliver` callback is the production one, and triage runs for real
//! against a backend that is not there, so it fails fast and the delivery is
//! retried rather than acted on.
//!
//! Run: `cargo test -p openhuman-cli --features linked-modules --test inbound_linked_e2e`

#[path = "support/linked_modules.rs"]
mod linked_modules;

use std::time::Duration;

use openhuman_core::config::schema::ModuleSourceMode;
use openhuman_core::config::Config;
use openhuman_core::inbound::{events, route};
use openhuman_core::modules::types::{ModuleSource, ModuleState};
use openhuman_core::modules::{event_queues, ops};
use openhuman_core::triage::TriggerEnvelope;
use tinyeventqueues_bus::Disposition;

fn config() -> Config {
    let mut config = Config::default();
    config.modules.enabled = true;
    // A fallback to the release must fail loudly rather than download.
    config.modules.allow_download = false;
    config.modules.source = ModuleSourceMode::Linked;
    config.components.eventqueues = true;
    config.inbound.enabled = true;
    config
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn linked_tinyeventqueues_queues_dedupes_and_delivers_a_composio_trigger() {
    let workspace = tempfile::tempdir().expect("workspace tempdir");
    std::fs::write(
        workspace.path().join("config.toml"),
        "[inbound]\ntriage_timeout_ms = 1500\n",
    )
    .expect("seed config.toml");
    // This target runs one test in its own process; nothing else reads these.
    std::env::set_var("OPENHUMAN_WORKSPACE", workspace.path());
    std::env::set_var("BACKEND_URL", "http://127.0.0.1:9");
    let previous = openhuman_core::storage::install(std::sync::Arc::new(
        tinystoragedrivers_testkit::MemoryBackend::new(),
    ));
    assert!(previous.is_none());
    linked_modules::install();

    let config = config();
    let client = ops::client(config.clone());
    let envelope = TriggerEnvelope::from_composio(
        "gmail",
        "GMAIL_NEW_GMAIL_MESSAGE",
        "msg-1",
        "uuid-linked-1",
        serde_json::json!({ "subject": "linked e2e" }),
    );

    // Ingest: the module loads from the executable and persists the event.
    let event = events::composio(&envelope).expect("a composio trigger is queueable");
    let first = route::offer(&config, &client, event.clone()).await;
    assert_eq!(first, route::Offered::Queued(Disposition::Accepted));
    let status = ops::list(&config)
        .into_iter()
        .find(|module| module.id == event_queues::MODULE_ID)
        .expect("tinyeventqueues is registered");
    assert_eq!(status.state, ModuleState::Ready);
    assert_eq!(status.source, Some(ModuleSource::Linked));

    // A redelivery of the same trigger is rejected at ingress.
    let again = route::offer(&config, &client, event).await;
    assert_eq!(again, route::Offered::Queued(Disposition::Duplicate));

    // Deliver: the module's own timer moves the event to a lane and calls the
    // host's `Deliver`, which triages it (and, offline, asks for a retry). The
    // turn is leased while that runs, then delayed or settled.
    let delivered = tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let stats = event_queues::stats(&client).await.expect("stats");
            let lanes_touched = stats
                .queues
                .iter()
                .filter(|queue| queue.queue.starts_with("teq.lane."))
                .any(|queue| queue.leased + queue.delayed > 0);
            let settled = stats.outcomes.delivered + stats.outcomes.dead > 0;
            if lanes_touched || settled {
                return stats;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("the module delivered the trigger to the host");
    let ingress = delivered
        .queues
        .iter()
        .find(|queue| queue.queue == "teq.ingress")
        .expect("the ingress queue is reported");
    assert_eq!(ingress.ready, 0, "the module never took the ingress");
}
