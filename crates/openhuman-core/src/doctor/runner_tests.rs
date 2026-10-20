use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crate::config::Config;
use crate::core::runtime::{ComponentSet, CoreContext, DomainSet};
use crate::core::slots::Extensions;
use async_trait::async_trait;

use super::contract::{
    DoctorCheck, DoctorConfigSource, DoctorCtx, DoctorOptions, DoctorTier, Finding, FindingSeverity,
};
use super::registry::{register_check_source_in, DoctorCheckSource};
use super::runner;
use super::test_support::{FakeCheck, FakeCheckSource};

#[test]
fn no_unwritten_daemon_state_check_is_registered() {
    let checks = super::registry::registered_checks().expect("built-in checks are valid");

    assert!(
        checks.iter().all(|check| check.id() != "runtime/daemon"),
        "the doctor must not advertise a daemon state check whose file is never written"
    );
}

#[test]
fn live_embedding_and_memory_roundtrip_checks_are_registered_as_live() {
    let checks = super::registry::registered_checks().expect("built-in checks are valid");

    for id in ["embeddings/live", "memory/roundtrip"] {
        let check = checks
            .iter()
            .find(|check| check.id() == id)
            .unwrap_or_else(|| panic!("the live doctor check {id} must be registered"));
        assert_eq!(
            check.tier(),
            DoctorTier::Live,
            "{id} may run only when the doctor live tier is requested"
        );
    }
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
    assert!(report
        .findings
        .iter()
        .any(|finding| finding.check_id == "hosted/ready"));
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
    assert!(report
        .findings
        .iter()
        .any(|finding| finding.check_id == "inference/live/role"));
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
    assert!(report
        .findings
        .iter()
        .any(|finding| finding.check_id == "doctor/selection"
            && finding.severity == FindingSeverity::Error));
    assert_eq!(report.schema_version, 2);
    assert_eq!(report.summary.pass, 1);
    assert_eq!(report.summary.error, 1);
    assert!(!report.ok);
}
