use std::time::Duration;

use crate::config::Config;
use crate::core::runtime::{ComponentSet, CoreContext, DomainSet};
use crate::core::slots::Extensions;

use super::contract::{DoctorCtx, DoctorOptions, DoctorTier, FindingSeverity};
use super::registry::register_check_source_in;
use super::runner;
use super::test_support::{FakeCheck, FakeCheckSource};

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
            DoctorOptions::default(),
        )
        .await
    })
    .await
    .expect("registered check source runs");

    assert!(report.checks_run.iter().any(|id| id == "hosted/ready"));
    assert!(
        report
            .findings
            .iter()
            .any(|finding| finding.check_id == "hosted/ready")
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
                only: vec!["config/temperature".into(), "missing/id".into()],
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
