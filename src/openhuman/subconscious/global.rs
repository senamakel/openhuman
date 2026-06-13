//! Global singleton for the SubconsciousEngine.
//!
//! Shared between the heartbeat background loop and RPC handlers
//! so both see the same state and counters.

use super::engine::SubconsciousEngine;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

static ENGINE: OnceLock<Arc<Mutex<Option<SubconsciousEngine>>>> = OnceLock::new();
static BOOTSTRAPPED: AtomicBool = AtomicBool::new(false);
static HEARTBEAT_HANDLE: OnceLock<Mutex<Option<JoinHandle<()>>>> = OnceLock::new();

fn engine_lock() -> &'static Arc<Mutex<Option<SubconsciousEngine>>> {
    ENGINE.get_or_init(|| Arc::new(Mutex::new(None)))
}

fn heartbeat_slot() -> &'static Mutex<Option<JoinHandle<()>>> {
    HEARTBEAT_HANDLE.get_or_init(|| Mutex::new(None))
}

pub async fn get_or_init_engine() -> Result<Arc<Mutex<Option<SubconsciousEngine>>>, String> {
    let lock = engine_lock();
    {
        let guard = lock.lock().await;
        if guard.is_some() {
            return Ok(Arc::clone(lock));
        }
    }

    let config = crate::openhuman::config::Config::load_or_init()
        .await
        .map_err(|e| format!("load config: {e}"))?;

    let memory = crate::openhuman::memory_store::MemoryClient::from_workspace_dir(
        config.workspace_dir.clone(),
    )
    .ok()
    .map(Arc::new);

    let engine = SubconsciousEngine::new(&config, memory);

    let mut guard = lock.lock().await;
    if guard.is_none() {
        *guard = Some(engine);
    }

    Ok(Arc::clone(lock))
}

pub async fn bootstrap_after_login() -> Result<(), String> {
    if BOOTSTRAPPED.swap(true, Ordering::SeqCst) {
        tracing::debug!("[subconscious] bootstrap already ran — skipping");
        return Ok(());
    }

    let config = crate::openhuman::config::Config::load_or_init()
        .await
        .map_err(|e| {
            BOOTSTRAPPED.store(false, Ordering::SeqCst);
            format!("load config: {e}")
        })?;

    if !config.heartbeat.enabled {
        tracing::info!("[subconscious] heartbeat disabled in config — bootstrap skipped");
        BOOTSTRAPPED.store(false, Ordering::SeqCst);
        return Ok(());
    }

    get_or_init_engine().await.inspect_err(|_e| {
        BOOTSTRAPPED.store(false, Ordering::SeqCst);
    })?;
    tracing::info!(
        workspace = %config.workspace_dir.display(),
        "[subconscious] engine initialized against per-user workspace"
    );

    let heartbeat = crate::openhuman::heartbeat::engine::HeartbeatEngine::new(
        config.heartbeat.clone(),
        config.workspace_dir.clone(),
    );
    let handle = tokio::spawn(async move {
        if let Err(e) = heartbeat.run().await {
            tracing::warn!("[heartbeat] loop exited with error: {e}");
        }
    });
    *heartbeat_slot().lock().await = Some(handle);
    tracing::info!(
        "[heartbeat] periodic loop spawned ({}min interval)",
        config.heartbeat.interval_minutes
    );

    // Opt-in event-driven trigger pipeline. When disabled, the legacy
    // interval-only heartbeat path above is the whole story.
    if config.heartbeat.triggers_enabled {
        bootstrap_trigger_orchestrator(&config);
    }

    Ok(())
}

/// Spawn the background trigger orchestrator (event loop) and register its
/// bus subscriber. Idempotent via the orchestrator's process-global slot.
fn bootstrap_trigger_orchestrator(config: &crate::openhuman::config::Config) {
    use crate::openhuman::subconscious_triggers::{
        init_orchestrator, register_subconscious_triggers_subscriber, OrchestratorConfig,
        TriggerOrchestrator,
    };

    let mode = config.heartbeat.effective_subconscious_mode();
    let session = Arc::new(super::LongLivedSession::new(
        config.workspace_dir.clone(),
        mode,
    ));
    let orch_config = OrchestratorConfig {
        max_promotions_per_hour: config.heartbeat.max_promotions_per_hour,
        ..OrchestratorConfig::default()
    };
    let orchestrator = init_orchestrator(Arc::new(TriggerOrchestrator::new(session, orch_config)));
    register_subconscious_triggers_subscriber(orchestrator);
    tracing::info!(
        workspace = %config.workspace_dir.display(),
        mode = %mode.as_str(),
        max_promotions_per_hour = config.heartbeat.max_promotions_per_hour,
        "[subconscious_triggers] event-driven orchestrator bootstrapped"
    );
}

pub async fn stop_heartbeat_loop() {
    if let Some(handle) = heartbeat_slot().lock().await.take() {
        handle.abort();
        match handle.await {
            Ok(()) => {
                tracing::debug!("[heartbeat] loop exited before abort completed");
            }
            Err(join_err) if join_err.is_cancelled() => {
                tracing::info!("[heartbeat] loop aborted");
            }
            Err(join_err) => {
                tracing::warn!(error = %join_err, "[heartbeat] loop abort join failed");
            }
        }
    }

    BOOTSTRAPPED.store(false, Ordering::SeqCst);
}

pub async fn reset_engine_for_user_switch() {
    stop_heartbeat_loop().await;

    // Tear down the event-driven trigger orchestrator + its bus subscriber so a
    // re-bootstrap binds the new user's workspace instead of routing through the
    // stale session.
    crate::openhuman::subconscious_triggers::shutdown_orchestrator();
    crate::openhuman::subconscious_triggers::unregister_subconscious_triggers_subscriber();

    let lock = engine_lock();
    let mut guard = lock.lock().await;
    *guard = None;

    tracing::info!("[subconscious] engine reset for user switch");
}
