//! [`run`] and [`run_models`]: the doctor report's public entry points.

use anyhow::Result;

use crate::config::Config;

use super::config_checks::check_config_semantics;
use super::daemon_env_checks::{check_daemon_state, check_environment};
use super::memory_agent_checks::{
    check_claude_agent_sdk, check_embedding_model_health, check_memory_tree_db,
};
use super::types::{
    DiagnosticItem, DoctorReport, DoctorSummary, ModelProbeEntry, ModelProbeOutcome,
    ModelProbeReport, ModelProbeSummary, Severity,
};
use super::workspace_checks::check_workspace;

/// How many chunks the bound memory driver holds, or why the count could not
/// be taken.
///
/// The one probe [`run`] cannot take for itself. The count comes from
/// `MemoryMaintenance::store_stats` — driver-neutral, and the reason this
/// check no longer opens the store's SQLite file directly (#5560) — and that
/// member is `async`, while [`run`] is blocking by contract. Blocking on it
/// from inside [`run`] is not an option in either direction: a
/// `Handle::block_on` panics on a current-thread runtime and deadlocks the
/// multi-thread one it is already occupying a worker of.
///
/// So the caller awaits it and hands the answer down. `Err` carries the
/// driver's own message and becomes the `Error` item this check has always
/// pushed when the probe failed.
pub type MemoryChunkCount = Result<u64, String>;

/// Build the full doctor report.
///
/// `ops::doctor_report` runs this in `tokio::task::spawn_blocking` because the
/// checks are synchronous and may touch the file system, sqlite, or local HTTP
/// endpoints. Keep this function blocking-only; add async probes in the caller
/// or behind their own runtime boundary instead of introducing `.await` here —
/// `memory_chunks` is the first probe to take that route, and `ops`'
/// `memory_chunk_count` is the shape to copy for the next one.
pub fn run(config: &Config, memory_chunks: MemoryChunkCount) -> Result<DoctorReport> {
    let mut items: Vec<DiagnosticItem> = Vec::new();

    check_config_semantics(config, &mut items);
    check_workspace(config, &mut items);
    check_daemon_state(config, &mut items);
    check_environment(&mut items);
    check_memory_tree_db(config, &memory_chunks, &mut items);
    check_embedding_model_health(config, &mut items);
    check_claude_agent_sdk(config, &mut items);

    let errors = items
        .iter()
        .filter(|i| i.severity == Severity::Error)
        .count();
    let warnings = items
        .iter()
        .filter(|i| i.severity == Severity::Warn)
        .count();
    let ok = items.iter().filter(|i| i.severity == Severity::Ok).count();

    Ok(DoctorReport {
        items,
        summary: DoctorSummary {
            ok,
            warnings,
            errors,
        },
    })
}

fn doctor_model_targets() -> Vec<String> {
    crate::inference::provider::list_providers()
        .into_iter()
        .map(|provider| provider.name.to_string())
        .collect()
}

pub fn run_models(_config: &Config, _use_cache: bool) -> Result<ModelProbeReport> {
    let targets = doctor_model_targets();

    if targets.is_empty() {
        anyhow::bail!("No providers available for model probing");
    }

    let skipped_count = targets.len();
    let entries = targets
        .into_iter()
        .map(|provider| ModelProbeEntry {
            provider,
            outcome: ModelProbeOutcome::Skipped,
            message: Some("model catalog refresh removed".to_string()),
        })
        .collect();

    Ok(ModelProbeReport {
        entries,
        summary: ModelProbeSummary {
            ok: 0,
            skipped: skipped_count,
            auth_or_access: 0,
            errors: 0,
        },
    })
}
