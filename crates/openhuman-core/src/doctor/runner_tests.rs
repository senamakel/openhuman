use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use crate::config::Config;
use crate::core::runtime::{ComponentSet, CoreContext, DomainSet};
use crate::core::slots::Extensions;
use async_trait::async_trait;

use super::contract::{
    DoctorCheck, DoctorConfigSource, DoctorCtx, DoctorOptions, DoctorTier, Finding, FindingSeverity,
};
use super::registry::{DoctorCheckSource, register_check_source_in};
use super::runner;
use super::test_support::{FakeCheck, FakeCheckSource};

struct DownMemoryEngine {
    inner: tinymemory_api::conformance::ReferenceEngine,
    reason: String,
}

#[async_trait]
impl tinymemory_api::MemoryEngine for DownMemoryEngine {
    fn descriptor(&self) -> &tinymemory_api::EngineDescriptor {
        self.inner.descriptor()
    }

    async fn health(&self) -> tinymemory_api::EngineHealth {
        tinymemory_api::EngineHealth::Down(self.reason.clone())
    }

    async fn recall(
        &self,
        request: tinymemory_api::RecallRequest,
    ) -> tinymemory_api::Result<tinymemory_api::RecallAnswer> {
        self.inner.recall(request).await
    }

    async fn fetch(
        &self,
        request: tinymemory_api::FetchRequest,
    ) -> tinymemory_api::Result<tinymemory_api::FetchPage> {
        self.inner.fetch(request).await
    }

    async fn store(
        &self,
        item: tinymemory_api::StoreItem,
    ) -> tinymemory_api::Result<tinymemory_api::StoreReceipt> {
        self.inner.store(item).await
    }

    async fn forget(
        &self,
        target: tinymemory_api::ForgetTarget,
    ) -> tinymemory_api::Result<tinymemory_api::ForgetReport> {
        self.inner.forget(target).await
    }

    async fn list(
        &self,
        request: tinymemory_api::ListRequest,
    ) -> tinymemory_api::Result<tinymemory_api::ListPage> {
        self.inner.list(request).await
    }
}

fn snapshot_test_tree(
    root: &std::path::Path,
) -> std::collections::BTreeMap<std::path::PathBuf, Option<Vec<u8>>> {
    fn walk(
        current: &std::path::Path,
        root: &std::path::Path,
        snapshot: &mut std::collections::BTreeMap<std::path::PathBuf, Option<Vec<u8>>>,
    ) {
        for entry in std::fs::read_dir(current).expect("read fixture directory") {
            let path = entry.expect("fixture entry").path();
            let relative = path
                .strip_prefix(root)
                .expect("fixture path is inside its root")
                .to_path_buf();
            if path.is_dir() {
                snapshot.insert(relative, None);
                walk(&path, root, snapshot);
            } else {
                snapshot.insert(
                    relative,
                    Some(std::fs::read(path).expect("read fixture file")),
                );
            }
        }
    }

    let mut snapshot = std::collections::BTreeMap::new();
    walk(root, root, &mut snapshot);
    snapshot
}

struct BlockingProbe {
    id: String,
    timeout: Duration,
    active: Arc<AtomicUsize>,
    peak: Arc<AtomicUsize>,
}

#[async_trait]
impl DoctorCheck for BlockingProbe {
    fn id(&self) -> &str {
        &self.id
    }

    fn category(&self) -> &str {
        "test"
    }

    fn tier(&self) -> DoctorTier {
        DoctorTier::Connect
    }

    fn timeout(&self) -> Duration {
        self.timeout
    }

    fn is_blocking(&self) -> bool {
        true
    }

    async fn detect(&self, _ctx: &DoctorCtx) -> Vec<Finding> {
        Vec::new()
    }

    fn detect_blocking(&self, _ctx: &DoctorCtx) -> Option<Vec<Finding>> {
        let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.peak.fetch_max(active, Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(500));
        self.active.fetch_sub(1, Ordering::SeqCst);
        Some(vec![Finding::pass(
            &self.id,
            "The blocking fake check completed.",
        )])
    }
}

struct BlockingProbeSource(Vec<Arc<dyn DoctorCheck>>);

impl DoctorCheckSource for BlockingProbeSource {
    fn id(&self) -> &str {
        "doctor-blocking-probes"
    }

    fn checks(&self) -> Vec<Arc<dyn DoctorCheck>> {
        self.0.clone()
    }
}

#[tokio::test]
async fn a_context_check_source_is_reported_with_its_check_id() {
    let extensions = Extensions::new();
    register_check_source_in(
        &extensions,
        FakeCheckSource::new(
            "doctor-runner-source",
            vec![FakeCheck::pass("hosted/ready")],
        ),
    );
    let context = CoreContext::for_test_with_extensions(DomainSet::full(), None, extensions);
    let report = CoreContext::scope(context, async {
        runner::run(
            &DoctorCtx::from_config(Config::default()),
            DoctorOptions {
                only: vec!["hosted/ready".into()],
                ..DoctorOptions::default()
            },
        )
        .await
    })
    .await
    .expect("registered check source runs");

    let config_source: DoctorConfigSource = report.config_source;
    assert_eq!(config_source, DoctorConfigSource::Profile);
    assert!(report.checks_run.iter().any(|id| id == "hosted/ready"));
    assert!(
        report
            .findings
            .iter()
            .any(|finding| finding.check_id == "hosted/ready")
    );
}

#[tokio::test]
async fn hierarchical_check_ids_are_registered_and_selected_exactly() {
    let extensions = Extensions::new();
    register_check_source_in(
        &extensions,
        FakeCheckSource::new(
            "doctor-hierarchical-id-source",
            vec![FakeCheck::pass("inference/live/role")],
        ),
    );
    let context = CoreContext::for_test_with_extensions(DomainSet::full(), None, extensions);
    let report = CoreContext::scope(context, async {
        runner::run(
            &DoctorCtx::from_config(Config::default()),
            DoctorOptions {
                tier: DoctorTier::Live,
                only: vec!["inference/live/role".into()],
                ..DoctorOptions::default()
            },
        )
        .await
    })
    .await
    .expect("hierarchical id is valid and selected as one exact id");

    assert_eq!(report.checks_run, vec!["inference/live/role".to_owned()]);
    assert!(
        report
            .findings
            .iter()
            .any(|finding| finding.check_id == "inference/live/role")
    );
}

#[tokio::test]
async fn a_timed_out_check_is_an_error_without_stopping_other_checks() {
    let extensions = Extensions::new();
    let slow = FakeCheck::pass("test/slow")
        .sleep_for(Duration::from_millis(100))
        .with_timeout(Duration::from_millis(10));
    register_check_source_in(
        &extensions,
        FakeCheckSource::new(
            "doctor-timeout-source",
            vec![slow, FakeCheck::pass("test/fast")],
        ),
    );
    let context = CoreContext::for_test_with_extensions(DomainSet::full(), None, extensions);
    let report = CoreContext::scope(context, async {
        runner::run(
            &DoctorCtx::from_config(Config::default()),
            DoctorOptions {
                only: vec!["test/slow".into(), "test/fast".into()],
                ..DoctorOptions::default()
            },
        )
        .await
    })
    .await
    .expect("timed out detector is returned as a finding");

    assert!(report.findings.iter().any(|finding| {
        finding.check_id == "test/slow"
            && finding.severity == FindingSeverity::Error
            && finding.message.contains("timed out")
    }));
    assert!(report.findings.iter().any(|finding| {
        finding.check_id == "test/fast" && finding.severity == FindingSeverity::Pass
    }));
}

#[tokio::test]
async fn timed_out_blocking_checks_keep_the_concurrency_slot_until_the_worker_exits() {
    let active = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    let ids = (0..16)
        .map(|index| format!("test/blocking-{index}"))
        .collect::<Vec<_>>();
    let checks = ids
        .iter()
        .map(|id| {
            Arc::new(BlockingProbe {
                id: id.clone(),
                timeout: Duration::from_millis(20),
                active: Arc::clone(&active),
                peak: Arc::clone(&peak),
            }) as Arc<dyn DoctorCheck>
        })
        .collect();
    let extensions = Extensions::new();
    register_check_source_in(&extensions, BlockingProbeSource(checks));
    let context = CoreContext::for_test_with_extensions(DomainSet::full(), None, extensions);
    let report = CoreContext::scope(context, async {
        runner::run(
            &DoctorCtx::from_config(Config::default()),
            DoctorOptions {
                only: ids,
                ..DoctorOptions::default()
            },
        )
        .await
    })
    .await
    .expect("all blocking checks return timed-out findings");

    assert!(
        active.load(Ordering::SeqCst) > 0,
        "the report should return while timed-out blocking workers are still finishing"
    );
    tokio::time::timeout(Duration::from_secs(3), async {
        while active.load(Ordering::SeqCst) != 0 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("blocking workers eventually finish");
    assert_eq!(report.summary.error, 16);
    assert!(peak.load(Ordering::SeqCst) <= 8);
}

#[tokio::test]
async fn memory_checks_are_skipped_when_the_memory_component_is_disabled() {
    let mut components = ComponentSet::all();
    components.memory = false;
    let context = DoctorCtx::from_config(Config::default()).with_components(components);
    let report = runner::run(
        &context,
        DoctorOptions {
            tier: DoctorTier::Connect,
            only: vec!["memory/engine".into()],
            ..DoctorOptions::default()
        },
    )
    .await
    .expect("component-gated check is represented as skipped");

    assert_eq!(report.checks_skipped, vec!["memory/engine".to_owned()]);
    assert!(report.findings.iter().any(|finding| {
        finding.check_id == "memory/engine" && finding.severity == FindingSeverity::Skipped
    }));
}

#[tokio::test]
async fn memory_health_check_reports_the_engine_down_reason() {
    let temp = tempfile::tempdir().expect("temp directory");
    let config = crate::memory::test_fixtures::config_in(&temp);
    let reason = "the configured memory service is unavailable";
    crate::memory::engine::install_test_engine(
        &config.workspace_dir,
        Arc::new(DownMemoryEngine {
            inner: tinymemory_api::conformance::ReferenceEngine::new(),
            reason: reason.to_owned(),
        }),
    );

    let report = runner::run(
        &DoctorCtx::from_config(config.clone()),
        DoctorOptions {
            only: vec!["memory/health".into()],
            ..DoctorOptions::default()
        },
    )
    .await
    .expect("memory health result is represented in the report");
    crate::memory::engine::remove_test_engine(&config.workspace_dir);

    let finding = report
        .findings
        .iter()
        .find(|finding| finding.check_id == "memory/health")
        .expect("the selected memory health check returns its own finding");
    assert_eq!(finding.severity, FindingSeverity::Error);
    assert!(finding.message.contains(reason));
}

#[tokio::test]
async fn storage_connect_check_does_not_change_the_selected_sqlite_tree() {
    let temp = tempfile::tempdir().expect("temp directory");
    let workspace = temp.path().join("workspace");
    std::fs::create_dir_all(&workspace).expect("create workspace");
    let database_dir = temp.path().join("selected-storage");
    let url = format!("sqlite:{}", database_dir.display());
    let backend = crate::storage::open(&url)
        .await
        .expect("create a local SQLite fixture using the normal setup path");
    drop(backend);
    let before = snapshot_test_tree(temp.path());
    let config = Config {
        action_dir: workspace.clone(),
        workspace_dir: workspace,
        storage: crate::config::StorageConfig {
            url: Some(url),
            ..crate::config::StorageConfig::default()
        },
        ..Config::default()
    };

    let report = runner::run(
        &DoctorCtx::from_config(config),
        DoctorOptions {
            only: vec!["storage/connect".into()],
            ..DoctorOptions::default()
        },
    )
    .await
    .expect("storage check is represented in the report");
    let finding = report
        .findings
        .iter()
        .find(|finding| finding.check_id == "storage/connect")
        .expect("the selected storage check returns its own finding");
    assert_eq!(finding.severity, FindingSeverity::Pass);
    assert_eq!(before, snapshot_test_tree(temp.path()));
}

#[tokio::test]
async fn storage_connect_check_reports_corrupt_sqlite_without_changing_it() {
    let temp = tempfile::tempdir().expect("temp directory");
    let workspace = temp.path().join("workspace");
    std::fs::create_dir_all(&workspace).expect("create workspace");
    let file = temp.path().join("corrupt.db");
    std::fs::write(&file, b"not a sqlite database").expect("write corrupt database fixture");
    let before = snapshot_test_tree(temp.path());
    let config = Config {
        action_dir: workspace.clone(),
        workspace_dir: workspace,
        storage: crate::config::StorageConfig {
            url: Some(format!("sqlite:{}", file.display())),
            ..crate::config::StorageConfig::default()
        },
        ..Config::default()
    };

    let report = runner::run(
        &DoctorCtx::from_config(config),
        DoctorOptions {
            only: vec!["storage/connect".into()],
            ..DoctorOptions::default()
        },
    )
    .await
    .expect("storage failure is represented in the report");
    let finding = report
        .findings
        .iter()
        .find(|finding| finding.check_id == "storage/connect")
        .expect("the selected storage check returns its own finding");

    assert_eq!(finding.severity, FindingSeverity::Error);
    assert!(!finding.message.contains(file.to_string_lossy().as_ref()));
    assert_eq!(before, snapshot_test_tree(temp.path()));
}

#[tokio::test]
async fn storage_layout_check_warns_when_a_migration_is_pending() {
    let temp = tempfile::tempdir().expect("temp directory");
    let target = crate::storage::layout::Target::new(temp.path(), "u1");
    let workspace = target.workspace();
    std::fs::create_dir_all(&workspace).expect("create workspace");
    std::fs::create_dir_all(&target.profile_dir).expect("create profile directory");
    let config = Config {
        action_dir: workspace.clone(),
        config_path: target.profile_dir.join("config.toml"),
        workspace_dir: workspace,
        ..Config::default()
    };
    let state_path = target
        .profile_dir
        .join(crate::storage::layout::state::STATE_FILE);
    let error = crate::storage::layout::migrate(
        &target,
        &config,
        &crate::storage::layout::Options {
            crash_after: Some(crate::storage::layout::Step::Import),
            ..crate::storage::layout::Options::default()
        },
    )
    .await
    .expect_err("the fixture migration should stop after its import step");
    assert!(matches!(
        error,
        crate::storage::layout::LayoutError::InjectedCrash(crate::storage::layout::Step::Import)
    ));
    let before = std::fs::read(&state_path).expect("read migration state");

    let report = runner::run(
        &DoctorCtx::from_config(config),
        DoctorOptions {
            only: vec!["storage/layout".into()],
            ..DoctorOptions::default()
        },
    )
    .await
    .expect("layout check is represented in the report");

    let finding = report
        .findings
        .iter()
        .find(|finding| finding.check_id == "storage/layout")
        .expect("the selected layout check returns its own finding");
    assert_eq!(finding.severity, FindingSeverity::Warn);
    assert_eq!(
        before,
        std::fs::read(state_path).expect("migration state remains readable")
    );
}

#[tokio::test]
async fn checks_above_the_requested_tier_are_skipped() {
    let extensions = Extensions::new();
    register_check_source_in(
        &extensions,
        FakeCheckSource::new(
            "doctor-tier-source",
            vec![FakeCheck::pass("inference/role-completion").with_tier(DoctorTier::Live)],
        ),
    );
    let context = CoreContext::for_test_with_extensions(DomainSet::full(), None, extensions);
    let report = CoreContext::scope(context, async {
        runner::run(
            &DoctorCtx::from_config(Config::default()),
            DoctorOptions {
                tier: DoctorTier::Connect,
                only: vec!["inference/role-completion".into()],
                ..DoctorOptions::default()
            },
        )
        .await
    })
    .await
    .expect("above-tier check is represented as skipped");

    assert_eq!(
        report.checks_skipped,
        vec!["inference/role-completion".to_owned()]
    );
    assert_eq!(report.checks_run, Vec::<String>::new());
    assert!(report.findings.iter().any(|finding| {
        finding.check_id == "inference/role-completion"
            && finding.severity == FindingSeverity::Skipped
            && finding.duration_ms == 0
    }));
}

#[tokio::test]
async fn only_filters_exact_ids_and_unknown_ids_get_a_selection_finding() {
    let extensions = Extensions::new();
    register_check_source_in(
        &extensions,
        FakeCheckSource::new(
            "doctor-selection-source",
            vec![
                FakeCheck::pass("config/temperature"),
                FakeCheck::pass("test/other"),
            ],
        ),
    );
    let context = CoreContext::for_test_with_extensions(DomainSet::full(), None, extensions);
    let report = CoreContext::scope(context, async {
        runner::run(
            &DoctorCtx::from_config(Config::default()),
            DoctorOptions {
                only: vec![
                    "config/temperature".into(),
                    "test/other".into(),
                    "missing/id".into(),
                ],
                skip: vec!["test/other".into()],
                ..DoctorOptions::default()
            },
        )
        .await
    })
    .await
    .expect("unknown selection is a report finding");

    assert_eq!(report.checks_run, vec!["config/temperature".to_owned()]);
    assert!(
        report
            .findings
            .iter()
            .any(|finding| finding.check_id == "doctor/selection"
                && finding.severity == FindingSeverity::Error)
    );
    assert_eq!(report.schema_version, 2);
    assert_eq!(report.summary.pass, 1);
    assert_eq!(report.summary.error, 1);
    assert!(!report.ok);
}
