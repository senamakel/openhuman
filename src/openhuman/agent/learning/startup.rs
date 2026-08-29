//! Always-on learning subscriber wiring.
//!
//! Registers the Phase 2/3/4 learning subscribers on the global event bus:
//!
//! - **Phase 2** — the email-signature producer (reacts to
//!   `DocumentCanonicalized` events and emits Identity candidates into the
//!   learning buffer). Needs no memory client.
//! - **Phase 3** — the event-driven rebuild trigger plus the periodic 30-minute
//!   rebuild loop. Needs the global memory client.
//! - **Phase 4** — the `ProfileMdRenderer` (re-renders the five cache-derived
//!   `PROFILE.md` blocks on `CacheRebuilt`). Needs the global memory client.
//!
//! # Why this lives here (#5003)
//!
//! These three subscriptions used to be wired inside
//! `channels::runtime::startup::start_channels`. That function is a misnamed
//! process-wide bootstrap that `core::runtime::services::spawn_channels_service`
//! **skips entirely** when no chat integration is configured (or when
//! `OPENHUMAN_DISABLE_CHANNEL_LISTENERS` is set) — logging only at debug. As a
//! result, channel-less users silently got **no** learning at all.
//!
//! [`register_learning_subscribers`] is invoked from the always-on Platform
//! boot path (`core::jsonrpc::register_domain_subscribers`, the unconditional
//! `DomainGroup::Platform` block), where the memory client and workspace dir are
//! already available. Registration is idempotent, so both boot paths (and repeat
//! calls) install each subscriber exactly once.

use std::path::Path;
use std::sync::OnceLock;

use tinybus::SubscriptionHandle;

static EMAIL_SIG_HANDLE: OnceLock<Option<SubscriptionHandle>> = OnceLock::new();

/// Register the always-on learning subscribers on the global event bus.
///
/// Idempotent for any caller: every subscription is guarded by a process-wide
/// `OnceLock`, so wiring this from multiple boot paths (or calling it twice)
/// registers each subscriber exactly once. The returned `SubscriptionHandle`s
/// are intentionally leaked into statics so the subscriptions stay alive for the
/// lifetime of the process (same pattern as `TracingSubscriber`).
///
/// `workspace_dir` is the resolved workspace directory used by the
/// `ProfileMdRenderer` to locate `PROFILE.md`.
pub fn register_learning_subscribers(workspace_dir: std::path::PathBuf) {
    // Phase 2 learning producer: email-signature subscriber reacts to
    // DocumentCanonicalized events and emits Identity candidates into the
    // buffer. Needs no memory client, so it always registers.
    register_email_signature_once(&EMAIL_SIG_HANDLE, || {
        crate::openhuman::agent::learning::extract::signature::register_email_signature_subscriber()
    });

    // Phase 3 + Phase 4 learning: rebuild trigger + periodic loop + the
    // ProfileMdRenderer. All three need a bound memory driver. Readiness is
    // resolved here and the dependent work is split into `register_with_memory`
    // so both the ready and not-ready arms are unit-testable without touching
    // process globals.
    static CLIENT_HANDLES: OnceLock<(Option<SubscriptionHandle>, Option<SubscriptionHandle>)> =
        OnceLock::new();
    let memory_ready = memory_is_bindable(&workspace_dir);
    CLIENT_HANDLES.get_or_init(|| register_with_memory(memory_ready, &workspace_dir));
}

/// Whether this workspace has a usable memory driver to register learning
/// subscribers against.
///
/// This replaces `tinymemory_core::global::client_if_ready().is_some()` (#5560).
/// The two ask the same question — "is memory reachable yet?" — of different
/// things: the old call asked whether the in-process engine singleton had been
/// initialised, which after #5560 nothing does at boot, so it would have
/// answered `false` on every desktop start and silently taken learning down the
/// #5003 skip path forever.
///
/// `binding::for_workspace` is synchronous, cached, and infallible by design —
/// an inadmissible driver *falls back* rather than erroring — so the honest
/// negative here is `MemoryBinding::disables_memory`: memory explicitly
/// configured off, which is the one state in which a rebuild loop has nothing
/// to rebuild against.
fn memory_is_bindable(workspace_dir: &Path) -> bool {
    use crate::openhuman::config::schema::MemorySubsystemConfig;
    match crate::openhuman::memory::binding::for_workspace(
        workspace_dir,
        &MemorySubsystemConfig::default(),
    ) {
        Ok(binding) if binding.disables_memory() => {
            tracing::warn!(
                driver = %binding.driver_id(),
                "[learning::startup] memory is disabled for this workspace — learning subscribers will not register"
            );
            false
        }
        Ok(binding) => {
            tracing::debug!(
                driver = %binding.driver_id(),
                "[learning::startup] memory driver bound for learning subscribers"
            );
            true
        }
        Err(error) => {
            tracing::warn!("[learning::startup] no memory binding for this workspace: {error}");
            false
        }
    }
}

fn register_email_signature_once<F>(handle_cell: &OnceLock<Option<SubscriptionHandle>>, register: F)
where
    F: FnOnce() -> Option<SubscriptionHandle>,
{
    handle_cell.get_or_init(|| {
        let handle = register();
        if handle.is_some() {
            tracing::info!(
                "[learning] email-signature subscriber registered (channel-independent boot path)"
            );
        } else {
            tracing::warn!(
                "[learning] email-signature subscriber NOT registered — event bus not initialised"
            );
        }
        handle
    });
}

/// Register the client-dependent learning subscribers.
///
/// The profile facet cache for `workspace_dir`.
///
/// Resolved through the memory binding rather than the process-global client:
/// facets live behind the driver now, and `binding::for_workspace` is
/// synchronous and cached, so this stays callable from the boot path without
/// an await.
fn facet_cache_for(
    workspace_dir: &std::path::Path,
) -> Option<crate::openhuman::agent::learning::cache::FacetCache> {
    use crate::openhuman::config::schema::MemorySubsystemConfig;
    match crate::openhuman::memory::binding::for_workspace(
        workspace_dir,
        &MemorySubsystemConfig::default(),
    ) {
        Ok(binding) => Some(crate::openhuman::agent::learning::cache::FacetCache::new(
            binding.guard(),
        )),
        Err(error) => {
            tracing::warn!("[learning::startup] no memory binding for facet cache: {error}");
            None
        }
    }
}

/// Returns `(rebuild_trigger_handle, profile_md_renderer_handle)`.
///
/// When `memory_ready` is true, both the Phase 3 rebuild trigger (plus its
/// periodic 30-minute loop) and the Phase 4 `ProfileMdRenderer` are registered.
/// When it is false (this workspace has no usable memory driver) both are
/// skipped and the skip is logged at **warn** — the *silent* skip was the #5003
/// bug, so this must be loud.
///
/// Taking readiness as a parameter (rather than resolving the binding
/// internally) keeps both arms testable without touching process globals; it was
/// an `Option<MemoryClientRef>` for the same reason before #5560, and the client
/// itself was never used for anything but its presence.
fn register_with_memory(
    memory_ready: bool,
    workspace_dir: &Path,
) -> (Option<SubscriptionHandle>, Option<SubscriptionHandle>) {
    if !memory_ready {
        tracing::warn!(
            "[learning::scheduler] no memory driver for this workspace — skipping event-trigger + \
             periodic-rebuild registration; learning rebuilds will not fire (#5003)"
        );
        tracing::warn!(
            "[learning::profile_md_renderer] no memory driver for this workspace — skipping \
             ProfileMdRenderer registration; PROFILE.md will not be re-rendered (#5003)"
        );
        return (None, None);
    }

    // Phase 3 learning: event-driven rebuild trigger + periodic 30-minute loop.
    let rebuild_trigger = {
        use crate::openhuman::agent::learning::scheduler::register_event_trigger;
        use crate::openhuman::agent::learning::StabilityDetector;
        use std::sync::Arc;
        let Some(cache) = facet_cache_for(workspace_dir) else {
            return (None, None);
        };
        let detector = Arc::new(StabilityDetector::new(cache));
        // Also spawn the periodic rebuild loop (30-minute cadence).
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        // Leak the sender so the loop never receives a shutdown signal until the
        // process exits. This matches the pattern used by other always-on
        // background tasks.
        Box::leak(Box::new(shutdown_tx));
        crate::openhuman::agent::learning::scheduler::spawn_rebuild_loop(
            Arc::clone(&detector),
            crate::openhuman::agent::learning::scheduler::DEFAULT_REBUILD_INTERVAL,
            shutdown_rx,
        );
        let handle = register_event_trigger(detector);
        if handle.is_some() {
            tracing::info!(
                "[learning::scheduler] rebuild trigger + periodic loop registered \
                 (channel-independent boot path)"
            );
        }
        handle
    };

    // Phase 4 learning: ProfileMdRenderer subscribes to CacheRebuilt events and
    // re-renders the five cache-derived PROFILE.md blocks (style, identity,
    // tooling, vetoes, goals).
    let profile_md = {
        use crate::openhuman::agent::learning::ProfileMdRenderer;
        use std::sync::Arc;
        let Some(cache) = facet_cache_for(workspace_dir) else {
            return (rebuild_trigger, None);
        };
        let cache = Arc::new(cache);
        let renderer = Arc::new(ProfileMdRenderer::new(cache, workspace_dir.to_path_buf()));
        let handle = ProfileMdRenderer::subscribe(renderer);
        if handle.is_some() {
            tracing::info!(
                "[learning::profile_md_renderer] ProfileMdRenderer registered \
                 (channel-independent boot path)"
            );
        }
        handle
    };

    (rebuild_trigger, profile_md)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::events::DomainEvent;
    use crate::openhuman::agent::learning::candidate::Buffer;
    use crate::openhuman::agent::learning::extract::signature::{
        parse_signature, register_email_signature_subscriber_on,
    };
    use std::time::Duration;
    use tempfile::TempDir;

    /// A fresh temp workspace for the ready-arm test.
    ///
    /// This used to build a real `MemoryClient` and hand it to
    /// `register_with_client`, which never used it for anything but its
    /// presence. Readiness is a `bool` now (#5560), so the fixture is just the
    /// directory — but the host seams still have to be installed, because
    /// `memory_is_bindable` and the facet cache below both resolve a driver and
    /// an unwired embedding host fails loudly by design. `install_for_tests` is
    /// `Once`-guarded, so calling it here is free when another test already has.
    fn test_workspace() -> TempDir {
        crate::openhuman::memory::host_impls::install_for_tests();
        TempDir::new().expect("tempdir")
    }

    /// A body whose trailing lines form a clear email signature — yields several
    /// Identity candidates (name/role/timezone/employer).
    fn signature_body() -> String {
        "Hi, great to hear from you!\n\n\
         Thanks,\n\
         Alice Johnson\n\
         Senior Software Engineer\n\
         Acme Corp\n\
         San Francisco, CA\n\
         PST"
        .to_string()
    }

    fn email_doc(source_id: &str, body: &str) -> DomainEvent {
        DomainEvent::DocumentCanonicalized {
            source_id: source_id.to_string(),
            source_kind: "email".to_string(),
            chunks_written: 1,
            chunk_ids: vec![format!("{source_id}-c1")],
            canonicalized_at: 0.0,
            body_preview: Some(body.to_string()),
        }
    }

    /// Poll an isolated buffer until at least `expected` candidates appear,
    /// then settle briefly so an accidental duplicate subscription surfaces.
    async fn wait_for_candidates(buffer: &Buffer, expected: usize) -> usize {
        for _ in 0..50 {
            if buffer.len() >= expected {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
        buffer.len()
    }

    #[tokio::test]
    async fn register_with_memory_registers_both_handles_when_ready() {
        crate::core::bus::init().await.expect("bus init");
        let tmp = test_workspace();
        let (trigger, renderer) = register_with_memory(true, tmp.path());
        assert!(
            trigger.is_some(),
            "rebuild trigger must register when memory is available"
        );
        assert!(
            renderer.is_some(),
            "ProfileMdRenderer must register when memory is available"
        );
    }

    #[tokio::test]
    async fn register_with_memory_skips_and_warns_when_memory_absent() {
        // No memory driver → both memory-dependent subscribers are skipped and
        // the (now loud) warn path is exercised. This is the else-arm the #5003
        // fix upgraded from a silent debug-level skip.
        let tmp = TempDir::new().expect("tempdir");
        let (trigger, renderer) = register_with_memory(false, tmp.path());
        assert!(trigger.is_none(), "no trigger without a memory driver");
        assert!(renderer.is_none(), "no renderer without a memory driver");
    }

    #[tokio::test]
    async fn learning_subscriber_fires_with_no_channel_configured() {
        let bus = crate::core::bus_testing::isolated_bus().await;
        let buffer: &'static Buffer = Box::leak(Box::new(Buffer::new(16)));
        let handle_cell = OnceLock::new();
        register_email_signature_once(&handle_cell, || {
            Some(register_email_signature_subscriber_on(&bus, buffer))
        });

        let source_id = "gmail:5003-e2e";
        let body = signature_body();
        let expected = parse_signature(&body, source_id, source_id).len();
        assert!(
            expected > 0,
            "signature body must yield at least one identity candidate"
        );

        bus.publish(email_doc(source_id, &body));
        let got = wait_for_candidates(buffer, expected).await;
        assert_eq!(
            got, expected,
            "email-signature subscriber must push the parsed identity candidates \
             with no channel configured anywhere (#5003)"
        );
    }

    #[tokio::test]
    async fn register_learning_subscribers_is_idempotent() {
        let bus = crate::core::bus_testing::isolated_bus().await;
        let buffer: &'static Buffer = Box::leak(Box::new(Buffer::new(16)));
        let handle_cell = OnceLock::new();
        register_email_signature_once(&handle_cell, || {
            Some(register_email_signature_subscriber_on(&bus, buffer))
        });
        register_email_signature_once(&handle_cell, || {
            Some(register_email_signature_subscriber_on(&bus, buffer))
        });

        let source_id = "gmail:5003-idem";
        let body = signature_body();
        let expected = parse_signature(&body, source_id, source_id).len();
        assert!(expected > 0);

        bus.publish(email_doc(source_id, &body));
        let got = wait_for_candidates(buffer, expected).await;
        assert_eq!(
            got, expected,
            "double registration must not double the pushed candidates (#5003 idempotency)"
        );
    }
}
