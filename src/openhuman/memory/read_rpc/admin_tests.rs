//! Sibling tests for the admin read-RPCs, covering what moved onto the memory
//! contract in #5560.
//!
//! The three handlers here are the destructive ones, so what is worth pinning
//! host-side is not that a delete deleted — that is the driver's, and the
//! conformance suite proves it against a real store — but **how this host
//! answers when the bound driver cannot serve the family at all**. On a read,
//! a missing family degrades to an honest empty answer; on a wipe or a delete
//! it must not, because a caller reading "nothing was removed" concludes the
//! content was already gone. Each refusal below is that distinction.

use super::{clear_composio_sync_state, delete_source_rpc, wipe_all_rpc};
use crate::openhuman::config::Config;
use crate::openhuman::memory::api::provider::MemoryProvider;
use crate::openhuman::memory::binding;
use std::sync::Arc;
use tempfile::TempDir;
use tinymemory_api::null::NullMemoryProvider;

fn test_config() -> (TempDir, Config) {
    let tmp = TempDir::new().unwrap();
    let mut cfg = Config::default();
    cfg.workspace_dir = tmp.path().to_path_buf();
    // Keep any persistence these handlers trigger inside the disposable
    // workspace, exactly as `read_rpc_tests::test_config` does.
    cfg.config_path = tmp.path().join("config.toml");
    cfg.memory_tree.embedding_endpoint = None;
    cfg.memory_tree.embedding_model = None;
    cfg.memory_tree.embedding_strict = false;
    (tmp, cfg)
}

/// Bind the placeholder driver, which advertises the mandatory three families
/// and returns `None` from every optional `as_*` accessor. That absence is the
/// whole point of the driver, and it is what these tests are about.
fn install_null_driver(cfg: &Config) {
    binding::install_for_test(
        &cfg.workspace_dir,
        &cfg.subsystems.memory,
        Arc::new(NullMemoryProvider::new()) as Arc<dyn MemoryProvider>,
    );
}

/// A wipe a driver cannot perform is an error, never a `rows_deleted: 0`
/// success — the caller's next act is telling a user their memory is gone.
/// This mirrors the contract's own choice to default `purge_all` to
/// `Unsupported` while the rest of its family defaults to an empty result.
#[tokio::test]
async fn wipe_all_refuses_a_driver_that_does_not_serve_maintenance() {
    let (_tmp, cfg) = test_config();
    install_null_driver(&cfg);

    let err = wipe_all_rpc(&cfg)
        .await
        .expect_err("a driver with no Maintenance family cannot report a completed wipe");
    assert!(
        err.contains("does not serve Maintenance"),
        "the refusal must name the missing family: {err}"
    );
    assert!(
        err.contains("null"),
        "the refusal must name the driver that refused: {err}"
    );
}

/// The opposite call in the same handler, and deliberately not a refusal.
///
/// This replaced a `path.exists()` check that skipped silently, and the number
/// it returned then is the number it returns now: a driver with no key/value
/// tier has nothing of this host's to clear, so zero is a true statement about
/// it rather than a fault a caller can act on.
#[tokio::test]
async fn clear_composio_sync_state_reports_zero_without_a_graph_family() {
    let (_tmp, cfg) = test_config();
    install_null_driver(&cfg);

    assert_eq!(
        clear_composio_sync_state(&cfg)
            .await
            .expect("a driver with no Graph family is not an error"),
        0
    );
}

/// Same reasoning as the wipe: on a delete, a zero the caller reads as
/// "already gone" is worse than a refusal.
#[tokio::test]
async fn delete_source_refuses_a_driver_that_does_not_serve_sources() {
    let (_tmp, cfg) = test_config();
    install_null_driver(&cfg);

    let err = delete_source_rpc(&cfg, "notion:page-a".to_string())
        .await
        .expect_err("a driver with no Sources family cannot report a completed delete");
    assert!(
        err.contains("does not serve Sources"),
        "the refusal must name the missing family: {err}"
    );
}

/// Argument validation stays ahead of the driver lookup, so a blank id is a
/// caller error naming itself rather than a round trip that deletes nothing.
/// No driver is installed here on purpose — reaching one would be the failure.
#[tokio::test]
async fn delete_source_rejects_a_blank_source_id_before_touching_a_driver() {
    let (_tmp, cfg) = test_config();

    let err = delete_source_rpc(&cfg, "   ".to_string())
        .await
        .expect_err("a blank source id is a caller error");
    assert!(
        err.contains("must be a non-empty string"),
        "unexpected error: {err}"
    );
}

/// An unknown source removes nothing and cleans no tree, and the host maps that
/// all-zero `ForgetOutcome` onto `deleted: false`.
///
/// The mapping is the whole point of the assertion: `deleted` is now an OR over
/// **two** counts, and a source that matched nothing must not read as one whose
/// stranded summary tree was swept.
#[tokio::test]
async fn delete_source_is_idempotent_for_an_unknown_source_id() {
    let (_tmp, cfg) = test_config();
    crate::openhuman::memory::test_support::install_tinycortex_for_test(&cfg);

    let outcome = delete_source_rpc(&cfg, "notion:never-ingested".to_string())
        .await
        .expect("an unknown source is not an error")
        .value;
    assert!(!outcome.deleted);
    assert_eq!(outcome.chunks_removed, 0);
}

/// The source id can embed user-linked identifiers, so it is hashed into the
/// log line rather than written out. Pinned here because the log is assembled
/// beside the response and is easy to "improve" into a leak.
#[tokio::test]
async fn delete_source_never_logs_the_raw_source_id() {
    let (_tmp, cfg) = test_config();
    crate::openhuman::memory::test_support::install_tinycortex_for_test(&cfg);

    let source_id = "notion:alice@example.com/private-page";
    let outcome = delete_source_rpc(&cfg, source_id.to_string())
        .await
        .expect("an unknown source is not an error");
    assert!(
        !outcome.logs[0].contains(source_id),
        "log leaked the source id: {}",
        outcome.logs[0]
    );
    assert!(
        outcome.logs[0].contains("source_id_hash="),
        "log should carry the redacted id: {}",
        outcome.logs[0]
    );
}
