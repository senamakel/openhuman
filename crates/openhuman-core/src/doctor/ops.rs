//! JSON-RPC / CLI controller surface for diagnostics.
//!
//! Resolve async inputs for the temporary v1 adapters, then run the v2 check
//! registry and report assembler.

use crate::config::Config;
use crate::core::Outcome;
use crate::doctor::{
    DoctorCtx, DoctorOptions, DoctorReport, DoctorTier, MemoryEngineCheck, ModelProbeReport,
};

pub async fn doctor_report(config: &Config) -> Result<Outcome<DoctorReport>, String> {
    doctor_report_with_options(config, DoctorOptions::default()).await
}

pub async fn doctor_report_with_options(
    config: &Config,
    options: DoctorOptions,
) -> Result<Outcome<DoctorReport>, String> {
    let ctx = DoctorCtx::from_config(config.clone());
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
    _config: &Config,
    _use_cache: bool,
) -> Result<Outcome<ModelProbeReport>, String> {
    Err("Model catalog probing is not available yet.".to_owned())
}
