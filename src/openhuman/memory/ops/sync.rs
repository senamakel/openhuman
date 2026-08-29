//! Memory-sync RPC handlers and ingestion-status reporting.
//!
//! Sync RPCs publish `DomainEvent::MemorySyncRequested` on the global event
//! bus — they are fire-and-forget hooks for future ingestion subscribers.
//!
//! # One engine call is left here, and it is an openhuman#5560 blocker
//!
//! - **`spawn_manual_sync` → `tinycortex::run_composio_connection`.** The
//!   engine's own `run_composio_connection_with_caps` opens with
//!   `global::client_if_ready().ok_or(… "memory client is not ready")`, so with
//!   the in-process engine gone every target fails and this handler emits
//!   `MemorySyncStage::Failed` per connection. Loud, at least, but wrong. There
//!   is no contract member to move to: the whole pipeline is `tinycortex`-shaped
//!   (a `SyncPipeline` over provider-specific fetchers), and the loaded module
//!   does not run it either — `tinymemory` v1.5.0's module carries a section
//!   headed "The periodic sync loops are deliberately NOT started here" with
//!   three named reasons. Manual sync and the periodic loop share this call, so
//!   they move together or not at all, and the ordering that imposes lives next
//!   to the loop it protects in
//!   [`memory::sync::composio`](crate::openhuman::memory::sync::composio).
//!
//! # `memory_ingestion_status` was the quiet one, and it is fixed
//!
//! It used to read the in-process engine's live `IngestionState` through
//! `global::client_if_ready()`, whose `None` arm answered "idle, queue empty" —
//! indistinguishable from a healthy store with nothing to do. Once the second
//! engine stopped booting, that arm became the *only* arm: the RPC reported
//! permanent idle regardless of reality, and the Memory panel's ingestion
//! indicator never lit up again. The frontend polls this every 1.5–4s
//! (`useMemoryIngestionStatus`, `useBackgroundActivity`), so the wrong answer
//! was on screen continuously.
//!
//! It now reads `MemoryMaintenance::queue_stats` off the bound driver, which is
//! the contract's own queue telemetry and needs no new bus member. See
//! `ingestion_status_for_config` for exactly which fields survived that move
//! and which the contract does not carry.

use crate::openhuman::config::rpc as config_rpc;
use crate::openhuman::config::Config;
use crate::openhuman::memory::sync::composio;
use crate::rpc::RpcOutcome;
use tinymemory_api::sync_events::{emit_sync_stage, MemorySyncStage, MemorySyncTrigger};

/// Parameters for `memory_sync_channel`.
#[derive(Debug, serde::Deserialize)]
pub struct SyncChannelParams {
    pub channel_id: String,
}

/// Result returned by `memory_sync_channel`.
#[derive(Debug, serde::Serialize)]
pub struct SyncChannelResult {
    pub requested: bool,
    pub channel_id: String,
}

/// Result returned by `memory_sync_all`.
#[derive(Debug, serde::Serialize)]
pub struct SyncAllResult {
    pub requested: bool,
}

/// Result returned by `memory_ingestion_status` — the public RPC shape, kept
/// deliberately separate from whatever the driver answers so an internal rename
/// cannot break the wire contract.
///
/// **Five fields are permanently `None` since the move onto the contract**
/// (`current_document_id`, `current_title`, `current_namespace`,
/// `last_document_id`, `last_success`) — see `ingestion_status_for_config`,
/// which documents the reduction field by field. They stay on the struct because
/// `MemoryControls`, `OverviewPanel` and `useBackgroundActivity` decode this
/// shape and every one of them is `skip_serializing_if = "Option::is_none"`
/// already, so an absent field is a shape the frontend has always handled.
/// Removing them would be a wire change; leaving them is not a promise that
/// they are filled.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct IngestionStatusResult {
    pub running: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_document_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_namespace: Option<String>,
    pub queue_depth: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_completed_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_document_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_success: Option<bool>,
}

/// Request a memory sync for a specific channel.
///
/// Ingestion in OpenHuman is listener/webhook-driven — there is no per-provider
/// pull mechanism yet. This RPC publishes `DomainEvent::MemorySyncRequested` so
/// that future ingestion subscribers can react to an explicit pull request.
/// The event is fire-and-forget; the caller receives confirmation that the
/// request was published, not that ingestion ran.
pub async fn memory_sync_channel(
    params: SyncChannelParams,
) -> Result<RpcOutcome<SyncChannelResult>, String> {
    // `channel_id` is a user/context identifier — keep it out of normal logs.
    tracing::info!("[memory.sync] memory_sync_channel: entry");
    crate::core::bus::BUS.publish(crate::core::events::DomainEvent::MemorySyncRequested {
        channel_id: Some(params.channel_id.clone()),
    });
    emit_sync_stage(
        MemorySyncTrigger::Manual,
        MemorySyncStage::Requested,
        None,
        Some(&params.channel_id),
        Some("channel-targeted sync requested".to_string()),
        None, // channel-level sync — not a memory-source row
    );
    let channel_id_for_spawn = params.channel_id.clone();
    tokio::spawn(async move {
        if let Err(e) = spawn_manual_sync(Some(channel_id_for_spawn)).await {
            tracing::warn!(error = %e, "[memory.sync] background channel sync failed");
        }
    });
    tracing::debug!("[memory.sync] memory_sync_channel: MemorySyncRequested published");
    Ok(RpcOutcome::new(
        SyncChannelResult {
            requested: true,
            channel_id: params.channel_id,
        },
        vec![],
    ))
}

/// Request a memory sync for all channels.
///
/// Publishes `DomainEvent::MemorySyncRequested { channel_id: None }` on the
/// global event bus. No consumers exist yet — this is a hook for future
/// ingestion subscribers.
pub async fn memory_sync_all() -> Result<RpcOutcome<SyncAllResult>, String> {
    tracing::info!("[memory.sync] memory_sync_all: entry");
    crate::core::bus::BUS
        .publish(crate::core::events::DomainEvent::MemorySyncRequested { channel_id: None });
    emit_sync_stage(
        MemorySyncTrigger::Manual,
        MemorySyncStage::Requested,
        None,
        None,
        Some("global sync requested".to_string()),
        None, // global sync — not a memory-source row
    );
    tokio::spawn(async move {
        if let Err(e) = spawn_manual_sync(None).await {
            tracing::warn!(error = %e, "[memory.sync] background global sync failed");
        }
    });
    tracing::debug!("[memory.sync] memory_sync_all: MemorySyncRequested(all) published");
    Ok(RpcOutcome::new(SyncAllResult { requested: true }, vec![]))
}

async fn spawn_manual_sync(requested_connection: Option<String>) -> Result<(), String> {
    let config = config_rpc::load_config_with_timeout().await?;
    let targets = match composio::list_sync_targets(&config).await {
        Ok(t) => t,
        Err(e) => {
            tracing::debug!(
                error = %e,
                "[memory.sync] no composio sync targets available — proceeding with empty list"
            );
            Vec::new()
        }
    };

    let targets: Vec<composio::SyncTarget> = match requested_connection.as_deref() {
        Some(requested) => targets
            .into_iter()
            .filter(|target| target.connection_id == requested || target.toolkit == requested)
            .collect(),
        None => targets,
    };

    if let Some(requested) = requested_connection.as_deref() {
        if targets.is_empty() {
            emit_sync_stage(
                MemorySyncTrigger::Manual,
                MemorySyncStage::Failed,
                None,
                Some(requested),
                Some("no active provider-backed sync target matched request".to_string()),
                None, // channel-level sync — not a memory-source row
            );
            return Err(format!(
                "memory sync: no active provider-backed target matched `{requested}`"
            ));
        }
    }

    // Resolved BEFORE the spawn so a missing driver is an error the caller
    // sees, not a status line the spawned task emits into a channel nobody is
    // reading yet.
    let binding = crate::openhuman::memory::binding::for_config(&config)?;

    tokio::spawn(async move {
        for target in targets {
            emit_sync_stage(
                MemorySyncTrigger::Manual,
                MemorySyncStage::Fetching,
                Some(&target.toolkit),
                Some(&target.connection_id),
                Some("provider sync started".to_string()),
                None, // provider-level composio sync — not a memory-source row
            );

            // Through the driver, not the engine. `run_connection_sync` drops
            // the config argument the engine call took: the driver resolves its
            // own, and its proxied branch now reaches this host for the session
            // bearer rather than reading a snapshot that has none.
            let outcome = match binding.provider().as_source_sync() {
                Some(sync) => sync
                    .run_connection_sync(&target.toolkit, &target.connection_id)
                    .await
                    .map_err(|error| error.to_string()),
                None => Err(format!(
                    "the bound memory driver '{}' does not serve source sync",
                    binding.driver_id()
                )),
            };
            match outcome {
                Ok(outcome) => {
                    emit_sync_stage(
                        MemorySyncTrigger::Manual,
                        MemorySyncStage::Completed,
                        Some(&target.toolkit),
                        Some(&target.connection_id),
                        Some(format!(
                            "provider sync completed items_ingested={}",
                            outcome.records_ingested
                        )),
                        None, // provider-level composio sync — not a memory-source row
                    );
                }
                Err(error) => {
                    emit_sync_stage(
                        MemorySyncTrigger::Manual,
                        MemorySyncStage::Failed,
                        Some(&target.toolkit),
                        Some(&target.connection_id),
                        Some(error.clone()),
                        None, // provider-level composio sync — not a memory-source row
                    );
                    tracing::warn!(
                        toolkit = %target.toolkit,
                        connection_id = %target.connection_id,
                        error = %error,
                        "[memory.sync] provider sync failed"
                    );
                }
            }
        }
    });

    Ok(())
}

/// Returns the current memory-ingestion status: whether the driver's queue is
/// working, how much is waiting, and when it last settled a job. Read-only,
/// safe to poll.
pub async fn memory_ingestion_status() -> Result<RpcOutcome<IngestionStatusResult>, String> {
    let config = config_rpc::load_config_with_timeout().await?;
    let status = ingestion_status_for_config(&config).await?;
    Ok(RpcOutcome::new(status, vec![]))
}

/// The queue half of [`memory_ingestion_status`], against an explicit config.
///
/// Split out so the mapping below is testable against a bound driver without
/// the handler's ambient `Config::load_or_init` — the same shape
/// `read_rpc::chunks` uses for its own paged reads.
///
/// # What moved, and what the contract does not carry
///
/// This was `global::client_if_ready().ingestion_state().snapshot()` — an
/// in-process counter owned by the engine this host no longer boots. The
/// replacement is
/// [`MemoryMaintenance::queue_stats`](crate::openhuman::memory::api::provider::MemoryMaintenance::queue_stats),
/// the contract's own queue
/// telemetry, so it works against *whichever* driver is bound rather than only
/// against an engine living in this address space.
///
/// | RPC field | Source | Note |
/// | --- | --- | --- |
/// | `running` | `QueueStats::running > 0` | jobs a worker currently holds |
/// | `queue_depth` | `QueueStats::ready` | jobs waiting; the running one is counted by `running`, exactly as the engine counter split them |
/// | `last_completed_at` | `QueueStats::last_completed_ms` | when the queue last settled a job |
///
/// **The reduction, stated rather than hidden.** `current_document_id`,
/// `current_title`, `current_namespace`, `last_document_id` and `last_success`
/// have no contract equivalent and are left `None`. `QueueStats` is counts, not
/// job identity: it can say a worker is busy, not *what* it is busy with. That
/// is a narrower answer than the engine's snapshot gave — and a strictly better
/// one than what shipped, because since the second engine stopped booting those
/// fields were `None` **and** `running`/`queue_depth` were falsely zero. Nothing
/// is substituted for them: `latest_queue_failure()` could be squinted at to
/// synthesise a `last_success`, and it would be a different question's answer
/// (the newest *terminal failure*, not the newest job's outcome).
///
/// Widening this back out is a contract member and a `tinymemory` release, not
/// a host change.
///
/// # Degrade
///
/// A driver error propagates — reporting "idle" because the driver failed is
/// the exact bug this replaced. A driver that does not serve `Maintenance` at
/// all reports zeros with the driver named in the log, matching its siblings in
/// `memory::tree::tree::rpc`: it has no queue to be behind on.
async fn ingestion_status_for_config(config: &Config) -> Result<IngestionStatusResult, String> {
    let binding = crate::openhuman::memory::binding::for_config(config)?;
    let Some(maintenance) = binding.provider().as_maintenance() else {
        log::debug!(
            "[memory.sync] ingestion_status: driver '{}' does not serve Maintenance; reporting idle",
            binding.driver_id()
        );
        return Ok(IngestionStatusResult::default());
    };

    // `None` counts every job kind. The engine counter this replaces was not
    // narrowed to the ingest kind either — it was bumped on every submit — so
    // narrowing here would silently shrink a number the Memory panel already
    // displays.
    let queue = maintenance
        .queue_stats(None)
        .await
        .map_err(|e| format!("queue_stats: {e}"))?;

    log::debug!(
        "[memory.sync] ingestion_status: driver='{}' running={} ready={} eligible_now={}",
        binding.driver_id(),
        queue.running,
        queue.ready,
        queue.eligible_now
    );

    Ok(IngestionStatusResult {
        running: queue.running > 0,
        current_document_id: None,
        current_title: None,
        current_namespace: None,
        queue_depth: usize::try_from(queue.ready).unwrap_or(usize::MAX),
        last_completed_at: queue.last_completed_ms,
        last_document_id: None,
        last_success: None,
    })
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, OnceLock};

    use super::*;
    use async_trait::async_trait;
    use serde_json::json;
    use tokio::sync::mpsc;
    use tokio::time::{timeout, Duration};

    use crate::core::bus::BUS;
    use crate::core::events::DomainEvent;
    use tinybus::EventHandler;

    fn test_mutex() -> &'static std::sync::Mutex<()> {
        static LOCK: OnceLock<std::sync::Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| std::sync::Mutex::new(()))
    }

    /// A config on its own temp workspace with a driver bound that reports
    /// `queue` verbatim.
    ///
    /// The ingestion-status handler reads through the contract now, and the
    /// real driver is a compiled module a unit test cannot load — so without a
    /// binding installed the workspace resolves to the null driver and every
    /// count answers zero, which is exactly the failure mode this handler was
    /// fixed for. See `binding::FixedDiagnostics`.
    fn bind_queue(
        queue: crate::openhuman::memory::api::provider::types::QueueStats,
    ) -> (tempfile::TempDir, Config) {
        let tmp = tempfile::TempDir::new().expect("tempdir");
        let mut config = Config::default();
        config.workspace_dir = tmp.path().to_path_buf();
        crate::openhuman::memory::binding::install_diagnostics_for_test(
            &config.workspace_dir,
            &config.subsystems.memory,
            Default::default(),
            queue,
        );
        (tmp, config)
    }

    struct ChannelCapture {
        tx: mpsc::UnboundedSender<Option<String>>,
    }

    #[async_trait]
    impl EventHandler<DomainEvent> for ChannelCapture {
        fn name(&self) -> &str {
            "memory::ops::sync::tests::capture"
        }

        fn domains(&self) -> Option<&[&str]> {
            Some(&["memory"])
        }

        async fn handle(&self, event: &DomainEvent) {
            if let DomainEvent::MemorySyncRequested { channel_id } = event {
                let _ = self.tx.send(channel_id.clone());
            }
        }
    }

    #[test]
    fn sync_channel_params_deserialize_channel_id() {
        let params: SyncChannelParams =
            serde_json::from_value(json!({"channel_id": "channel-1"})).unwrap();
        assert_eq!(params.channel_id, "channel-1");
    }

    #[test]
    fn ingestion_status_result_default_is_idle() {
        let status = IngestionStatusResult::default();
        assert!(!status.running);
        assert!(status.current_document_id.is_none());
        assert!(status.current_title.is_none());
        assert!(status.current_namespace.is_none());
        assert_eq!(status.queue_depth, 0);
        assert!(status.last_completed_at.is_none());
        assert!(status.last_document_id.is_none());
        assert!(status.last_success.is_none());
    }

    #[test]
    fn sync_result_structs_serialize_expected_fields() {
        let one = serde_json::to_value(SyncChannelResult {
            requested: true,
            channel_id: "abc".into(),
        })
        .unwrap();
        assert_eq!(one, json!({"requested": true, "channel_id": "abc"}));

        let all = serde_json::to_value(SyncAllResult { requested: true }).unwrap();
        assert_eq!(all, json!({"requested": true}));
    }

    #[tokio::test]
    async fn memory_sync_channel_publishes_targeted_event() {
        let _serial = crate::openhuman::memory::ops::GLOBAL_MEMORY_TEST_LOCK
            .lock()
            .await;
        let _guard = test_mutex()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let _ = crate::core::bus::init().await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        let _subscription = BUS
            .subscribe(Arc::new(ChannelCapture { tx }))
            .expect("global bus should be initialized");

        let outcome = memory_sync_channel(SyncChannelParams {
            channel_id: "channel-123".into(),
        })
        .await
        .expect("memory_sync_channel");
        assert!(outcome.value.requested);
        assert_eq!(outcome.value.channel_id, "channel-123");

        let received = timeout(Duration::from_secs(1), rx.recv())
            .await
            .expect("event should arrive before timeout")
            .expect("sender should still be connected");
        assert_eq!(received.as_deref(), Some("channel-123"));
    }

    #[tokio::test]
    async fn memory_sync_all_publishes_broadcast_event() {
        let _serial = crate::openhuman::memory::ops::GLOBAL_MEMORY_TEST_LOCK
            .lock()
            .await;
        let _guard = test_mutex()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let _ = crate::core::bus::init().await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        let _subscription = BUS
            .subscribe(Arc::new(ChannelCapture { tx }))
            .expect("global bus should be initialized");

        let outcome = memory_sync_all().await.expect("memory_sync_all");
        assert!(outcome.value.requested);

        let received = timeout(Duration::from_secs(1), rx.recv())
            .await
            .expect("event should arrive before timeout")
            .expect("sender should still be connected");
        assert!(
            received.is_none(),
            "sync-all should publish channel_id=None"
        );
    }

    /// The mapping from the driver's `QueueStats` onto the RPC shape, including
    /// the fields the contract does not carry.
    ///
    /// This replaces a test that drove the in-process engine's `IngestionState`
    /// counters directly. That test passed while the production RPC was
    /// answering permanent idle, because it initialised the very engine
    /// singleton production had stopped booting — the assertion held against a
    /// path nothing reached.
    #[tokio::test]
    async fn ingestion_status_reports_the_bound_drivers_queue() {
        let (_tmp, config) =
            bind_queue(crate::openhuman::memory::api::provider::types::QueueStats {
                ready: 3,
                running: 1,
                last_completed_ms: Some(1_700_000_000_000),
                ..Default::default()
            });

        let status = ingestion_status_for_config(&config)
            .await
            .expect("ingestion status");

        assert!(status.running, "a held job means the queue is working");
        assert_eq!(status.queue_depth, 3, "queue_depth is the ready count");
        assert_eq!(status.last_completed_at, Some(1_700_000_000_000));

        // The reduction, asserted rather than assumed: `QueueStats` is counts,
        // not job identity, so nothing fills these and nothing is invented for
        // them. If a future contract member does carry the in-flight document,
        // this is the test that should stop compiling as written.
        assert!(status.current_document_id.is_none());
        assert!(status.current_title.is_none());
        assert!(status.current_namespace.is_none());
        assert!(status.last_document_id.is_none());
        assert!(status.last_success.is_none());
    }

    /// An idle queue reports idle — the answer the broken handler used to give
    /// unconditionally, now given only when the driver actually says so.
    #[tokio::test]
    async fn ingestion_status_reports_idle_for_an_empty_queue() {
        let (_tmp, config) = bind_queue(Default::default());

        let status = ingestion_status_for_config(&config)
            .await
            .expect("ingestion status");

        assert!(!status.running);
        assert_eq!(status.queue_depth, 0);
        assert!(status.last_completed_at.is_none());
    }
}
