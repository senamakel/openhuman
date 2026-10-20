//! OpenHuman's scheduling and RPC adapter for Native layout migration.
//!
//! TinyMemory owns the migration job, policy, live slot and retry behavior.
//! This module supplies the host executor, workspace key, cross-driver gate,
//! billing-independent scheduler pause and wire error mapping.

use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock};

use tinymemory_tools::layout_migration as lm;
use tinymemory_tools::run_slots::{MigrationSpawner, MigrationTask, RunSlots};

use super::app_host::{from_module, AppHost};
use super::{LayoutHost, MigrationState, MigrationStatus, ScanView, Trigger};
use crate::config::Config;
use crate::core::runtime::DispatchCtx;
use crate::memory::run_slots::{self, BACKFILL, IMPORT, MIGRATION};
use tinymemory_api::Result;

static MIGRATION_SLOTS: LazyLock<RunSlots<PathBuf>> = LazyLock::new(RunSlots::new);

/// `memory_migration_start` params.
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct StartParams {
    /// The user's consent to take a legacy tree other accounts may share.
    #[serde(default)]
    pub takeover: bool,
}

struct ContextSpawner(DispatchCtx);

impl MigrationSpawner for ContextSpawner {
    fn spawn(&self, task: MigrationTask) -> std::result::Result<(), String> {
        self.0.spawn(task);
        Ok(())
    }
}

fn service(ctx: &DispatchCtx) -> lm::MigrationService<tinymemory_tools::run_slots::RunStartGate, ContextSpawner> {
    lm::MigrationService::new(
        MIGRATION_SLOTS.clone(),
        run_slots::START_GATE.clone(),
        ContextSpawner(ctx.clone()),
    )
}

/// Whether this workspace currently owns a migration task.
pub(crate) fn is_running(config: &Config) -> bool {
    MIGRATION_SLOTS.is_running(&config.workspace_dir)
}

/// Whether `host`'s account has legacy memory to move.
pub async fn scan(host: &dyn LayoutHost) -> Result<ScanView> {
    lm::scan(host).await.map_err(from_module)
}

/// The migration's progress for `config`'s workspace.
pub fn status(config: &Config) -> Result<MigrationStatus> {
    lm::status(&AppHost::new(config.clone()), is_running(config)).map_err(from_module)
}

/// Starts a migration; Native atomically checks peer activity and reserves the
/// migration-only run slot under the shared start gate.
pub fn start(
    ctx: &DispatchCtx,
    config: Config,
    host: Arc<dyn LayoutHost>,
    trigger: Trigger,
    paused: Arc<dyn Fn() -> bool + Send + Sync>,
) -> bool {
    service(ctx).start(
        config.workspace_dir.clone(),
        host,
        || IMPORT.is_running(&config.workspace_dir) || BACKFILL.is_running(&config.workspace_dir),
        trigger,
        paused,
    )
}

/// Puts failed migration items back in line.
pub fn retry(config: &Config) -> Result<MigrationState> {
    lm::retry(&AppHost::new(config.clone()), is_running(config)).map_err(from_module)
}

/// Memory's background job: starts or resumes an automatic run unless the
/// migration is done, paused, or already running.
pub fn tick(
    ctx: &DispatchCtx,
    config: &Config,
    host: Arc<dyn LayoutHost>,
    paused: Arc<dyn Fn() -> bool + Send + Sync>,
) -> bool {
    service(ctx).tick(
        config.workspace_dir.clone(),
        host,
        || IMPORT.is_running(&config.workspace_dir) || BACKFILL.is_running(&config.workspace_dir),
        paused,
    )
}

#[cfg(test)]
#[path = "ops_tests.rs"]
mod tests;
