//! Tests for the driver-backed handlers in [`super`].
//!
//! A sibling file rather than an inline `mod tests`, because the null-driver
//! fixture below names `NullMemoryProvider` and the memory-guard bypass scanner
//! skips `*_tests.rs` by path but not an inline module. Constructing a null
//! driver to prove a refusal names it is not a production bypass, and the list
//! that scanner guards is one that may shrink and must never grow — so the
//! honest fix is to put the test where the repo already puts tests.

use super::*;
use crate::core::subsystem::DriverClass;
// Needed to call the family accessors on the *concrete* null provider
// below; the handlers above reach them through `dyn MemoryProvider`, where
// the trait is in scope by construction.
use crate::openhuman::memory::api::provider::MemoryProvider;
use std::sync::Arc;
use tinymemory_api::null::{NullMemoryProvider, NULL_DRIVER_ID};

/// The refusal always names the bound driver. That is the whole contract of
/// this message: an operator reading it has to be able to tell "no driver
/// serves sync" apart from "the sync failed", and the driver id is what
/// carries the difference.
#[test]
fn the_refusal_names_the_bound_driver_and_the_family() {
    let binding = crate::openhuman::memory::binding::bind_provider_for_test(
        Arc::new(NullMemoryProvider::new()),
        DriverClass::Null,
    );
    let message = unserved(&binding, "source sync", "sync_audit_log");

    assert!(
        message.contains(NULL_DRIVER_ID),
        "the refusal must name the driver, got: {message}"
    );
    assert!(
        message.contains("source sync"),
        "the refusal must name the family, got: {message}"
    );
}

/// The null driver really does serve neither family — the premise every
/// `else` arm in this file rests on. A driver that started serving them
/// would make those arms unreachable, and silently.
#[test]
fn the_null_driver_serves_neither_family() {
    let provider = NullMemoryProvider::new();
    assert!(provider.as_source_sync().is_none());
    assert!(provider.as_coding_sessions().is_none());
}

/// openhuman#5820: the all-in sweep must aggregate per-source trigger
/// failures into the response instead of laundering them — in the incident
/// every source failed `no memory source registered` and the RPC still
/// answered a clean success with `sync_triggered: 0` and nothing else.
#[tokio::test]
async fn trigger_sweep_aggregates_failures_instead_of_laundering_them() {
    fn entry(id: &str, enabled: bool) -> MemorySourceEntry {
        serde_json::from_value(serde_json::json!({
            "id": id,
            "kind": "folder",
            "label": "Test",
            "enabled": enabled,
            "path": ".",
        }))
        .expect("a valid folder source entry")
    }

    let sources = vec![
        entry("ok_1", true),
        entry("src_gone", true),
        entry("disabled", false),
        entry("src_also_gone", true),
    ];

    // Succeeds for ids starting `ok`, refuses everything else the way the
    // incident did.
    let (triggered, errors) = trigger_enabled_syncs(&sources, |source_id| async move {
        if source_id.starts_with("ok") {
            Ok(())
        } else {
            Err(format!(
                "not found: no memory source registered as {source_id}"
            ))
        }
    })
    .await;

    assert_eq!(triggered, 1, "only the healthy enabled source triggers");
    assert_eq!(errors.len(), 2, "each failed enabled source is reported");
    assert!(errors[0].starts_with("src_gone: "), "got: {}", errors[0]);
    assert!(
        errors[0].contains("no memory source registered"),
        "the caller must see the real reason, got: {}",
        errors[0]
    );
    assert!(
        errors[1].starts_with("src_also_gone: "),
        "got: {}",
        errors[1]
    );
}
