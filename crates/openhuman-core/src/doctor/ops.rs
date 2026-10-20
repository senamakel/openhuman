//! JSON-RPC / CLI controller surface for diagnostics.
//!
//! Resolve async inputs for the temporary v1 adapters, then run the v2 check
//! registry and report assembler.

use super::contract::DoctorProfileStatus;
use crate::config::Config;
use crate::core::{
    runtime::{ComponentSet, CoreContext, DispatchCtx},
    Outcome,
};
use crate::doctor::{
    self, DoctorConfigSource, DoctorCtx, DoctorOptions, DoctorReport, DoctorSummary, DoctorTier,
    Finding, MemoryEngineCheck, ModelProbeReport,
};

pub async fn doctor_report(config: &Config) -> Result<Outcome<DoctorReport>, String> {
    doctor_report_with_options(config, DoctorOptions::default()).await
}

pub async fn doctor_report_with_options(
    config: &Config,
    mut options: DoctorOptions,
) -> Result<Outcome<DoctorReport>, String> {
    if let Some(profile_id) = options.profile.take() {
        let Some(root) = super::profile::root_for_config(config)
            .or_else(|| crate::config::default_root_openhuman_dir().ok())
        else {
            return Ok(profile_error_outcome(
                safe_profile_id(&profile_id),
                options.tier,
                super::profile::DesktopProfileError::Missing,
            ));
        };
        let selected = match super::profile::load_desktop_profile(&root, &profile_id).await {
            Ok(selected) => selected,
            Err(error) => {
                return Ok(profile_error_outcome(
                    safe_profile_id(&profile_id),
                    options.tier,
                    error,
                ));
            }
        };
        let components = ComponentSet::from_config(&selected.config.components);
        let ctx = DoctorCtx::from_config_with_components(selected.config.clone(), components)
            .with_profile(selected.id, DoctorConfigSource::Profile)
            .with_profile_metadata(Some(selected.layout.version), Some("--profile".into()))
            .with_profile_status(DoctorProfileStatus::Valid);
        return CoreContext::scope(
            CoreContext::bare(),
            report_with_context(&selected.config, options, ctx),
        )
        .await;
    }

    let mut ctx = DoctorCtx::from_config(config.clone());
    if ctx.profile().is_none() {
        if let Some(profile_id) = super::profile::id_for_config(config) {
            ctx = ctx.with_profile(profile_id, DoctorConfigSource::Profile);
        }
    }
    report_with_context(config, options, ctx).await
}

/// Runs a report under the dispatch's selected tenant context when present,
/// or resolves a desktop `--profile` request using the read-only loader.
pub async fn doctor_report_for_dispatch(
    dispatch: &DispatchCtx,
    mut options: DoctorOptions,
) -> Result<Outcome<DoctorReport>, String> {
    let target =
        crate::profiles::doctor::resolve_target(dispatch, options.profile.as_deref()).await?;
    if let Some(target) = target {
        let config = target.config.clone();
        let ctx =
            DoctorCtx::from_config_with_components(config.clone(), target.context.components())
                .with_profile(target.profile_id.to_string(), DoctorConfigSource::Tenant);
        options.profile = None;
        return CoreContext::scope(target.context, report_with_context(&config, options, ctx))
            .await;
    }

    let config = dispatch.load_config().await?;
    doctor_report_with_options(&config, options).await
}

async fn report_with_context(
    config: &Config,
    options: DoctorOptions,
    ctx: DoctorCtx,
) -> Result<Outcome<DoctorReport>, String> {
    let needs_memory_engine = ctx.components().memory
        && options.tier.includes(DoctorTier::Connect)
        && (options.only.is_empty() || options.only.iter().any(|id| id == "memory/engine"))
        && !options.skip.iter().any(|id| id == "memory/engine");
    let memory = if needs_memory_engine {
        memory_engine_check(config).await
    } else {
        MemoryEngineCheck {
            engine: None,
            status: "off".into(),
            reason: Some("memory engine check was not selected".into()),
        }
    };
    let ctx = ctx.with_memory_engine(memory);
    let report = super::runner::run(&ctx, options).await?;
    Ok(Outcome::single_log(report, "doctor report generated"))
}

fn profile_error_outcome(
    profile: Option<String>,
    tier: DoctorTier,
    error: super::profile::DesktopProfileError,
) -> Outcome<DoctorReport> {
    let findings = vec![Finding::error(
        "profile/exists",
        error.message(),
        "Select a supported desktop profile with a valid config and rerun doctor.",
    )];
    let summary = DoctorSummary::from_findings(&findings);
    Outcome::single_log(
        DoctorReport {
            schema_version: 2,
            profile,
            config_source: DoctorConfigSource::Profile,
            tier,
            checks_run: vec!["profile/exists".into()],
            checks_skipped: Vec::new(),
            findings,
            summary,
            ok: false,
        },
        "doctor profile selection failed",
    )
}

fn safe_profile_id(profile_id: &str) -> Option<String> {
    super::profile::is_profile_path_segment(profile_id).then(|| profile_id.to_owned())
}

/// The memory engine's status for the doctor report.
async fn memory_engine_check(config: &Config) -> MemoryEngineCheck {
    let view = crate::memory::ops::engine_get(config).await;
    let status = serde_json::to_value(view.status)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_else(|| "down".to_string());
    MemoryEngineCheck {
        engine: view.engine,
        status,
        reason: view.reason,
    }
}

pub async fn doctor_models(
    config: &Config,
    use_cache: bool,
) -> Result<Outcome<ModelProbeReport>, String> {
    let report = doctor::run_models(config, use_cache).map_err(|e| e.to_string())?;
    Ok(Outcome::single_log(report, "model probes completed"))
}
