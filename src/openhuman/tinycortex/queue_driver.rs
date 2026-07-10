//! Queue worker-loop driver seam (migration W4).
//!
//! TinyCortex owns the job *store* and the single-step engine
//! (`queue::run_once` claims one `mem_tree_jobs` row, dispatches it through
//! [`QueueDelegates`], and settles it) but deliberately drops the tokio worker
//! pool, the wall-clock scheduler, Sentry reporting, and the storage-degraded
//! state machine — those are host concerns (plan §1, deletion ledger: "host
//! worker loop + Sentry/degraded wiring kept host"). This seam is where the
//! host drives the crate queue.
//!
//! **This module is the first W4 brick: the host-retained error policy.** When
//! `run_once` returns an error, the legacy `memory_queue::worker` loop applied a
//! carefully-tuned "back off, don't page" policy per failure class — the product
//! of several Sentry floods (OPENHUMAN-TAURI-BP, #2206, TAURI-RUST-4R8/E93,
//! CORE-RUST-19J). [`classify_worker_error`] ports that decision table verbatim
//! on top of the crate's now-merged classifiers
//! ([`is_host_io_error`] etc., tinycortex#63), so the crate-driven loop
//! reproduces it exactly. It is a pure function so the policy is unit-tested
//! without spinning a live loop.
//!
//! Still to land in W4 (tracked in the migration spec): the real
//! `HostQueueDelegates` that bridges the 8 delegate methods to the host
//! `memory_tree` engine (paired with W5), the tokio worker-loop + scheduler that
//! applies this policy while driving `run_once`, re-pointing `memory/global.rs`
//! and the enqueue call sites onto `queue::store`, and deleting the legacy
//! `memory_queue` engine. This brick is additive: nothing is flipped yet.

use std::time::Duration;

use async_trait::async_trait;
use tinycortex::memory::queue::worker::{
    is_host_io_error, is_sqlite_busy, is_sqlite_corrupt, is_sqlite_disk_full, is_sqlite_io_transient,
};
use tinycortex::memory::queue::{
    AppendDecision, AppendTarget, ExtractDecision, NodeRef, QueueDelegates, ReembedProgress,
    SealDocumentPayload, SealPayload, StaleBuffer,
};
use tinycortex::memory::MemoryConfig;

use crate::openhuman::config::Config;
use crate::openhuman::memory::tree_source::get_or_create_source_tree;
use crate::openhuman::memory_store::chunks::store as chunk_store;
use crate::openhuman::memory_store::content as content_store;
use crate::openhuman::memory_store::trees::store as trees_store;
use crate::openhuman::memory_tree::tree::{bucket_seal, TreeFactory};

/// How the host worker loop should report an errored `run_once` poll to Sentry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerReport {
    /// Do not page — the condition is transient, or persistent-but-user-only-
    /// fixable and flood-prone (re-polling every second would bury the
    /// dashboard). The `log::warn!` breadcrumb is enough.
    Silent,
    /// Report exactly once via a process-wide latch keyed by this reason tag,
    /// then stay silent until the condition clears (so a genuinely-new later
    /// failure can still page once).
    Once(&'static str),
    /// Report every occurrence — a genuinely unexpected error that should keep
    /// surfacing.
    Always(&'static str),
}

/// The host-retained decision for an errored `run_once` poll: how long to back
/// off, whether/how to page, whether to flip the storage-degraded flag, and
/// whether to drive corrupt-DB quarantine+rebuild recovery.
///
/// Ported verbatim from the `memory_queue::worker` error arms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkerErrorAction {
    /// How long the worker sleeps before the next poll.
    pub backoff: Duration,
    /// Sentry reporting policy for this failure class.
    pub report: WorkerReport,
    /// Mark the memory_tree storage-degraded (`StorageUnavailable`) so the status
    /// panel shows the user an actionable "check your disk" banner — a persistent
    /// host-FS failure only the user can clear.
    pub mark_degraded: bool,
    /// Drive the corrupt-DB quarantine+rebuild recovery path (which owns its own
    /// report-once latch), rather than paging directly.
    pub recover_corrupt: bool,
}

/// Classify a `run_once` error into the host's back-off/report/degrade policy.
///
/// Mirrors the legacy `memory_queue::worker` arms exactly, on the crate's
/// classifiers:
/// - **busy/locked** (`SQLITE_BUSY`/`LOCKED`): 1s, silent — transient write-lock
///   contention that `busy_timeout` + the next poll almost always clears.
/// - **transient I/O** (`-shm` family, `CANTOPEN`, `IOERR_TRUNCATE`, breaker):
///   30s, silent (#2206 flooded ~19k events/4d).
/// - **disk full** (`SQLITE_FULL`): 300s, silent — persistent, user-only-fixable
///   (TAURI-RUST-4R8: ~95k events).
/// - **corrupt** (`SQLITE_CORRUPT`/`NOTADB`): 300s + quarantine/rebuild recovery
///   (which reports once) — never clears on its own (TAURI-RUST-E93).
/// - **host-FS** (EIO/ENOSPC/EROFS): 300s + storage-degraded + report-once —
///   failing/read-only storage (CORE-RUST-19J: ~10k events/50min).
/// - **anything else**: 1s + report-always — a genuine, unexpected error.
pub fn classify_worker_error(err: &anyhow::Error) -> WorkerErrorAction {
    if is_sqlite_busy(err) {
        WorkerErrorAction {
            backoff: Duration::from_secs(1),
            report: WorkerReport::Silent,
            mark_degraded: false,
            recover_corrupt: false,
        }
    } else if is_sqlite_io_transient(err) {
        WorkerErrorAction {
            backoff: Duration::from_secs(30),
            report: WorkerReport::Silent,
            mark_degraded: false,
            recover_corrupt: false,
        }
    } else if is_sqlite_disk_full(err) {
        WorkerErrorAction {
            backoff: Duration::from_secs(300),
            report: WorkerReport::Silent,
            mark_degraded: false,
            recover_corrupt: false,
        }
    } else if is_sqlite_corrupt(err) {
        WorkerErrorAction {
            backoff: Duration::from_secs(300),
            // The recovery path owns the report-once latch, so the classifier
            // itself stays silent and just requests recovery.
            report: WorkerReport::Silent,
            mark_degraded: false,
            recover_corrupt: true,
        }
    } else if is_host_io_error(err) {
        WorkerErrorAction {
            backoff: Duration::from_secs(300),
            report: WorkerReport::Once("tree_jobs_worker_host_io"),
            mark_degraded: true,
            recover_corrupt: false,
        }
    } else {
        WorkerErrorAction {
            backoff: Duration::from_secs(1),
            report: WorkerReport::Always("tree_jobs_worker"),
            mark_degraded: false,
            recover_corrupt: false,
        }
    }
}

/// Host implementation of the crate's [`QueueDelegates`] — the engine seam the
/// crate queue pushes its heavy per-job work through.
///
/// TinyCortex owns the job store + dispatch (`handle_job` parses payloads,
/// enqueues follow-ups, decides `Done`/`Defer`) but delegates the parts it
/// cannot do itself — scoring/admission, buffer pushes, sealing, embedding —
/// because they need `memory_tree` / `memory_store` internals that are host
/// (and, for tree/score, host until W5). This bridges each delegate method to
/// the existing host engine, holding the host [`Config`] the calls need (the
/// `&MemoryConfig` the crate passes is derived from this same workspace).
///
/// **Brick 2 status (additive — the driver is not flipped to this yet):** the
/// self-contained methods are wired to the real host engine; the LLM/embedding/
/// tree-mutation-heavy methods (`extract_chunk`, `append_node`, `seal_level`,
/// `seal_document`, `reembed_batch`) — which restructure the host handlers'
/// atomic-tx bodies into the crate's decision-returning shape — are staged for
/// brick 2b. They return an error rather than a silent no-op so a premature flip
/// fails loudly instead of dropping work.
pub struct HostQueueDelegates {
    config: Config,
}

impl HostQueueDelegates {
    /// Build the delegates over the host [`Config`] whose workspace the crate
    /// queue is driving.
    pub fn new(config: Config) -> Self {
        Self { config }
    }
}

#[async_trait]
impl QueueDelegates for HostQueueDelegates {
    async fn extract_chunk(
        &self,
        _config: &MemoryConfig,
        _chunk_id: &str,
    ) -> anyhow::Result<Option<ExtractDecision>> {
        anyhow::bail!("W4 brick 2b: extract_chunk engine bridge not yet ported")
    }

    async fn append_node(
        &self,
        _config: &MemoryConfig,
        _node: &NodeRef,
        _target: &AppendTarget,
    ) -> anyhow::Result<Option<AppendDecision>> {
        anyhow::bail!("W4 brick 2b: append_node engine bridge not yet ported")
    }

    /// Ported from `handle_seal`: seal exactly one buffer level. Returns `None`
    /// for the crate to enqueue as the parent — the host `seal_one_level`
    /// (called with `enqueue_follow_ups = true`) drives the cascade itself by
    /// enqueuing the summary's append + parent seal into the shared
    /// `mem_tree_jobs` table, which the crate's `run_once` then claims (identical
    /// schema, parity P4). A no-op (missing tree, empty buffer, gate not met)
    /// also returns `None`. *(Transitional: once the crate tree cascade is
    /// adopted in W5 this should switch to `enqueue_follow_ups = false` and
    /// return the parent `SealPayload` for the crate to enqueue.)*
    async fn seal_level(
        &self,
        _config: &MemoryConfig,
        payload: &SealPayload,
    ) -> anyhow::Result<Option<SealPayload>> {
        let Some(tree) = trees_store::get_tree(&self.config, &payload.tree_id)? else {
            return Ok(None);
        };
        let buf = trees_store::get_buffer(&self.config, &tree.id, payload.level)?;
        let forced = payload.force_now_ms.is_some();
        if buf.is_empty() || (!forced && !bucket_seal::should_seal(&buf)) {
            return Ok(None);
        }
        let strategy = TreeFactory::from_tree(&tree).label_strategy(&self.config);
        let summary_id =
            bucket_seal::seal_one_level(&self.config, &tree, &buf, &strategy, true).await?;
        // Best-effort: rewrite the sealed summary's on-disk obsidian tags. Entity
        // rows were committed inside seal_one_level, so they are visible here.
        if let Err(e) = content_store::update_summary_tags(&self.config, &summary_id) {
            log::warn!(
                "[tinycortex::queue_driver] update_summary_tags failed for summary_id={summary_id}: {e:#}"
            );
        }
        Ok(None)
    }

    /// Ported from `handle_flush_stale`: list L0/summary buffers older than
    /// `max_age_secs` that the crate should force-seal.
    async fn list_stale_buffers(
        &self,
        _config: &MemoryConfig,
        max_age_secs: i64,
    ) -> anyhow::Result<Vec<StaleBuffer>> {
        let cutoff = chrono::Utc::now() - chrono::Duration::seconds(max_age_secs);
        let buffers = trees_store::list_stale_buffers(&self.config, cutoff)?;
        Ok(buffers
            .into_iter()
            .map(|b| StaleBuffer {
                tree_id: b.tree_id,
                level: b.level,
            })
            .collect())
    }

    /// Ported from `handle_seal_document`: build/rebuild one document version's
    /// per-doc subtree and merge its doc-root into the connection tree.
    async fn seal_document(
        &self,
        _config: &MemoryConfig,
        payload: &SealDocumentPayload,
    ) -> anyhow::Result<()> {
        if payload.chunk_ids.is_empty() {
            return Ok(());
        }
        // One physical tree per connection scope (e.g. notion:{connection_id}).
        let tree = get_or_create_source_tree(&self.config, &payload.tree_scope)?;
        let strategy = TreeFactory::from_tree(&tree).label_strategy(&self.config);
        bucket_seal::seal_document_subtree(
            &self.config,
            &tree,
            &payload.doc_id,
            payload.version_ms,
            &payload.chunk_ids,
            &strategy,
        )
        .await?;
        Ok(())
    }

    async fn reembed_batch(
        &self,
        _config: &MemoryConfig,
        _signature: &str,
    ) -> anyhow::Result<ReembedProgress> {
        anyhow::bail!("W4 brick 2b: reembed_batch engine bridge not yet ported")
    }

    /// The active embedding-space signature the queue re-embed switch-path keys
    /// on — the config-derived `provider={};model={};dims={}` string (P10).
    fn active_signature(&self, _config: &MemoryConfig) -> String {
        chunk_store::tree_active_signature(&self.config)
    }

    /// Whether any chunk/summary still lacks a vector at `signature` — the
    /// coverage probe the re-embed backfill trigger uses (ported from
    /// `memory_queue::ops::ensure_reembed_backfill`).
    fn has_uncovered_reembed_work(
        &self,
        _config: &MemoryConfig,
        signature: &str,
    ) -> anyhow::Result<bool> {
        chunk_store::with_connection(&self.config, |conn| {
            Ok(chunk_store::has_uncovered_reembed_work(conn, signature)?)
        })
    }
}

#[cfg(test)]
mod tests {
    // Engine/queue types (`QueueDelegates`, `MemoryConfig`, the payload types,
    // `async_trait`) come through `super::*` from the module-level imports.
    use super::*;

    fn sqlite_failure(code: rusqlite::ErrorCode, extended: i32, msg: &str) -> anyhow::Error {
        anyhow::Error::from(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error {
                code,
                extended_code: extended,
            },
            Some(msg.into()),
        ))
    }

    #[test]
    fn busy_backs_off_one_second_silently() {
        let a = classify_worker_error(&sqlite_failure(
            rusqlite::ErrorCode::DatabaseBusy,
            5,
            "database is locked",
        ));
        assert_eq!(a.backoff, Duration::from_secs(1));
        assert_eq!(a.report, WorkerReport::Silent);
        assert!(!a.mark_degraded && !a.recover_corrupt);
    }

    #[test]
    fn transient_io_backs_off_thirty_seconds_silently() {
        let a = classify_worker_error(&sqlite_failure(
            rusqlite::ErrorCode::SystemIoFailure,
            1546,
            "disk I/O error",
        ));
        assert_eq!(a.backoff, Duration::from_secs(30));
        assert_eq!(a.report, WorkerReport::Silent);
    }

    #[test]
    fn disk_full_backs_off_long_and_silent() {
        let a = classify_worker_error(&sqlite_failure(
            rusqlite::ErrorCode::DiskFull,
            13,
            "database or disk is full",
        ));
        assert_eq!(a.backoff, Duration::from_secs(300));
        assert_eq!(a.report, WorkerReport::Silent);
        assert!(!a.mark_degraded && !a.recover_corrupt);
    }

    #[test]
    fn corrupt_drives_recovery_not_a_direct_page() {
        let a = classify_worker_error(&sqlite_failure(
            rusqlite::ErrorCode::DatabaseCorrupt,
            11,
            "database disk image is malformed",
        ));
        assert_eq!(a.backoff, Duration::from_secs(300));
        assert!(a.recover_corrupt, "corrupt must drive quarantine+rebuild");
        assert_eq!(
            a.report,
            WorkerReport::Silent,
            "recovery owns the report-once latch"
        );
        assert!(!a.mark_degraded);
    }

    #[test]
    fn host_io_marks_degraded_and_reports_once() {
        let a = classify_worker_error(&anyhow::Error::from(std::io::Error::from_raw_os_error(5)));
        assert_eq!(a.backoff, Duration::from_secs(300));
        assert!(a.mark_degraded, "host-FS failure must flip storage-degraded");
        assert_eq!(a.report, WorkerReport::Once("tree_jobs_worker_host_io"));
        assert!(!a.recover_corrupt);
    }

    #[test]
    fn unknown_error_reports_every_time_short_backoff() {
        let a = classify_worker_error(&anyhow::anyhow!("upstream returned 500"));
        assert_eq!(a.backoff, Duration::from_secs(1));
        assert_eq!(a.report, WorkerReport::Always("tree_jobs_worker"));
        assert!(!a.mark_degraded && !a.recover_corrupt);
    }

    /// A minimal host-side [`QueueDelegates`] — proves the host can satisfy the
    /// crate trait (all delegate arg/return types resolve) and that the host can
    /// drive `queue::run_once` end-to-end. The real engine bridge lands with the
    /// W4 delegates brick; this no-op stands in so the driver integration is
    /// exercised now.
    struct NoopDelegates;

    #[async_trait]
    impl QueueDelegates for NoopDelegates {
        async fn extract_chunk(
            &self,
            _config: &MemoryConfig,
            _chunk_id: &str,
        ) -> anyhow::Result<Option<ExtractDecision>> {
            Ok(None)
        }
        async fn append_node(
            &self,
            _config: &MemoryConfig,
            _node: &NodeRef,
            _target: &AppendTarget,
        ) -> anyhow::Result<Option<AppendDecision>> {
            Ok(None)
        }
        async fn seal_level(
            &self,
            _config: &MemoryConfig,
            _payload: &SealPayload,
        ) -> anyhow::Result<Option<SealPayload>> {
            Ok(None)
        }
        async fn list_stale_buffers(
            &self,
            _config: &MemoryConfig,
            _max_age_secs: i64,
        ) -> anyhow::Result<Vec<StaleBuffer>> {
            Ok(Vec::new())
        }
        async fn seal_document(
            &self,
            _config: &MemoryConfig,
            _payload: &SealDocumentPayload,
        ) -> anyhow::Result<()> {
            Ok(())
        }
        async fn reembed_batch(
            &self,
            _config: &MemoryConfig,
            _signature: &str,
        ) -> anyhow::Result<ReembedProgress> {
            Ok(ReembedProgress::Covered)
        }
        fn active_signature(&self, _config: &MemoryConfig) -> String {
            "provider=inert;model=none;dims=0".to_string()
        }
        fn has_uncovered_reembed_work(
            &self,
            _config: &MemoryConfig,
            _signature: &str,
        ) -> anyhow::Result<bool> {
            Ok(false)
        }
    }

    /// End-to-end smoke: the host can drive the crate queue. An empty workspace
    /// queue → `run_once` claims nothing → `Ok(false)`, and initialising the
    /// chunk DB along the way does not error.
    #[tokio::test]
    async fn host_drives_run_once_on_empty_queue() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let mc = MemoryConfig::new(tmp.path());
        let processed = tinycortex::memory::queue::run_once(&mc, &NoopDelegates)
            .await
            .expect("run_once on empty queue");
        assert!(!processed, "empty queue processes nothing");
    }

    fn host_delegates_on_tempdir() -> (tempfile::TempDir, HostQueueDelegates) {
        let tmp = tempfile::tempdir().expect("tempdir");
        let mut config = crate::openhuman::config::Config::default();
        config.workspace_dir = tmp.path().to_path_buf();
        (tmp, HostQueueDelegates::new(config))
    }

    /// The self-contained `HostQueueDelegates` methods bind to the real host
    /// engine and run on a fresh workspace: the signature is non-empty, and an
    /// empty workspace reports no uncovered re-embed work and no stale buffers.
    #[tokio::test]
    async fn host_delegates_selfcontained_methods_bind_and_run() {
        let (tmp, d) = host_delegates_on_tempdir();
        let mc = MemoryConfig::new(tmp.path());

        let sig = d.active_signature(&mc);
        assert!(!sig.is_empty(), "active signature should be non-empty");

        assert!(
            !d.has_uncovered_reembed_work(&mc, &sig)
                .expect("coverage probe"),
            "a fresh workspace has no uncovered re-embed work"
        );

        assert!(
            d.list_stale_buffers(&mc, 3600)
                .await
                .expect("list stale buffers")
                .is_empty(),
            "a fresh workspace has no stale buffers"
        );
    }

    /// The methods staged for brick 2b fail loudly (not a silent no-op) so a
    /// premature flip cannot drop work.
    #[tokio::test]
    async fn host_delegates_staged_methods_error_until_2b() {
        let (tmp, d) = host_delegates_on_tempdir();
        let mc = MemoryConfig::new(tmp.path());
        assert!(d.extract_chunk(&mc, "chunk-1").await.is_err());
        assert!(d
            .append_node(
                &mc,
                &NodeRef::Leaf {
                    chunk_id: "c".into()
                },
                &AppendTarget::Source {
                    source_id: "s".into()
                },
            )
            .await
            .is_err());
        assert!(d
            .reembed_batch(&mc, "provider=x;model=y;dims=3")
            .await
            .is_err());
    }

    /// The ported seal methods handle empty/missing state without error: an
    /// empty document version is a no-op, and sealing a level of a tree that
    /// doesn't exist yields no parent to cascade.
    #[tokio::test]
    async fn host_delegates_seal_methods_handle_empty_state() {
        let (tmp, d) = host_delegates_on_tempdir();
        let mc = MemoryConfig::new(tmp.path());

        d.seal_document(
            &mc,
            &SealDocumentPayload {
                tree_scope: "notion:conn".into(),
                doc_id: "notion:conn:page".into(),
                version_ms: Some(1),
                chunk_ids: vec![],
            },
        )
        .await
        .expect("seal_document on an empty version is a no-op");

        let parent = d
            .seal_level(
                &mc,
                &SealPayload {
                    tree_id: "nonexistent-tree".into(),
                    level: 0,
                    force_now_ms: None,
                },
            )
            .await
            .expect("seal_level on a missing tree");
        assert!(parent.is_none(), "missing tree has no parent to cascade");
    }
}
