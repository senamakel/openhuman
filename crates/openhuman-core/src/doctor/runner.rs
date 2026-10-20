//! Bounded async execution, filtering, and report assembly.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Instant;

use futures::stream::{self, StreamExt};
use tokio::sync::Semaphore;

use super::contract::{
    DoctorCtx, DoctorOptions, DoctorReport, DoctorSummary, Finding, FindingSeverity,
};
use super::registry;

const MAX_CONCURRENT_CHECKS: usize = 8;

/// Runs registered checks using exact-id filters and stable report ordering.
pub(super) async fn run(ctx: &DoctorCtx, options: DoctorOptions) -> Result<DoctorReport, String> {
    let checks = registry::registered_checks()?;
    let check_ids = checks
        .iter()
        .map(|check| check.id().to_owned())
        .collect::<HashSet<_>>();
    let only = options.only.into_iter().collect::<HashSet<_>>();
    let skip = options.skip.into_iter().collect::<HashSet<_>>();
    let has_unknown_selector = only
        .iter()
        .chain(skip.iter())
        .any(|id| !check_ids.contains(id));
    let _reserved_profile_input = options.profile;

    let mut evaluated = Vec::new();
    let mut runnable = Vec::new();

    for (index, check) in checks.into_iter().enumerate() {
        let id = check.id().to_owned();
        if (!only.is_empty() && !only.contains(&id)) || skip.contains(&id) {
            continue;
        }

        if !options.tier.includes(check.tier()) {
            evaluated.push((
                index,
                id.clone(),
                vec![Finding::skipped(
                    &id,
                    format!(
                        "The {} check is above the requested tier.",
                        check.category()
                    ),
                    "Request a higher doctor tier and rerun the check.",
                )],
                true,
            ));
            continue;
        }

        if let Some(component) = check.component_gate() {
            if !component.enabled(&ctx.components()) {
                let name = component.as_str();
                evaluated.push((
                    index,
                    id.clone(),
                    vec![Finding::skipped(
                        &id,
                        format!("The required {name} component is disabled."),
                        format!("Enable the {name} component and rerun doctor."),
                    )],
                    true,
                ));
                continue;
            }
        }
        runnable.push((index, check));
    }

    let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT_CHECKS));
    let mut outcomes = stream::iter(runnable)
        .map(|(index, check)| {
            let semaphore = Arc::clone(&semaphore);
            let ctx = ctx.clone();
            async move {
                let permit = semaphore
                    .acquire_owned()
                    .await
                    .expect("doctor semaphore is never closed");
                let id = check.id().to_owned();
                let timeout = check.timeout();
                let started = Instant::now();
                let detection = detect(Arc::clone(&check), ctx);
                let result = tokio::time::timeout(timeout, detection).await;
                drop(permit);
                let duration_ms = elapsed_ms(started.elapsed());
                let findings = match result {
                    Ok(Ok(mut findings)) => {
                        if findings.is_empty() {
                            findings
                                .push(Finding::pass(&id, "The check completed without findings."));
                        }
                        for finding in &mut findings {
                            finding.check_id.clone_from(&id);
                            finding.duration_ms = duration_ms;
                            if finding.fix_hint.trim().is_empty() {
                                finding.fix_hint =
                                    "Review the check result and rerun doctor.".into();
                            }
                        }
                        findings
                    }
                    Ok(Err(_)) => vec![Finding::error(
                        &id,
                        "The check worker failed.",
                        "Review the check's local dependencies and rerun doctor.",
                    )],
                    Err(_) => vec![Finding::error(
                        &id,
                        format!("The check timed out after {} ms.", timeout.as_millis()),
                        "Review the check's dependencies and rerun doctor.",
                    )],
                };
                (index, id, findings, false)
            }
        })
        .buffer_unordered(MAX_CONCURRENT_CHECKS)
        .collect::<Vec<_>>()
        .await;
    evaluated.append(&mut outcomes);
    evaluated.sort_by_key(|(index, _, _, _)| *index);

    let mut checks_run = Vec::new();
    let mut checks_skipped = Vec::new();
    let mut findings = Vec::new();
    for (_, id, mut check_findings, was_skipped) in evaluated {
        if was_skipped {
            checks_skipped.push(id);
        } else {
            checks_run.push(id);
        }
        findings.append(&mut check_findings);
    }

    if has_unknown_selector {
        findings.push(Finding::error(
            "doctor/selection",
            "One or more selected check ids are not registered.",
            "Use a registered exact check id from the doctor report.",
        ));
    }

    let summary = DoctorSummary::from_findings(&findings);
    let ok = summary.error == 0;
    Ok(DoctorReport {
        schema_version: 2,
        profile: ctx.profile().map(str::to_owned),
        config_source: ctx.config_source(),
        tier: options.tier,
        checks_run,
        checks_skipped,
        findings,
        summary,
        ok,
    })
}

async fn detect(
    check: Arc<dyn super::contract::DoctorCheck>,
    ctx: DoctorCtx,
) -> Result<Vec<Finding>, ()> {
    if check.is_blocking() {
        tokio::task::spawn_blocking(move || check.detect_blocking(&ctx).ok_or(()))
            .await
            .map_err(|_| ())?
    } else {
        Ok(check.detect(&ctx).await)
    }
}

fn elapsed_ms(duration: std::time::Duration) -> u64 {
    duration.as_millis().min(u64::MAX as u128) as u64
}
