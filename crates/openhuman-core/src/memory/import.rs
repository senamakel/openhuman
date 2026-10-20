//! Importing a v1 memory store into the selected engine.
//!
//! TinyMemory owns import batching, checkpoints, retry and backoff. This host
//! adapter keeps consent, the existing atomic checkpoint path, billing and
//! scheduler policy, executor context, and post-import layout migration.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use tinymemory_integrations::import::{
    CheckpointStore, ImportCounts as LegacyImportCounts, ImportError, ImportFile, ImportPhase,
    ImportState, Importer, Pacer, ResumePlan,
};

use crate::config::Config;
use crate::security::secret_storage::file_store::write_atomic;

use super::engine;
use super::run_slots::{self, BACKFILL, IMPORT, MIGRATION};
use super::types::{ImportCounts, ImportScanView};
use tinymemory_api::{Error, Result};

/// Whether background work is paused right now. An automatic resume asks
/// before it starts and again before every batch it stores.
pub(crate) type PauseCheck = Arc<dyn Fn() -> bool + Send + Sync>;

/// Whether memory work that uploads on its own may run now.
pub(crate) type BillingCheck = Arc<dyn Fn(&Config) -> bool + Send + Sync>;

/// The state file, written atomically (staged, synced, renamed): a crash
/// mid-write leaves the previous state, never a torn file that reads back as
/// a fresh start and re-sends everything.
pub(crate) struct FileCheckpointStore {
    path: PathBuf,
}

impl FileCheckpointStore {
    pub(crate) fn new(workspace_dir: &Path) -> Self {
        Self {
            path: workspace_dir.join("memory").join("import_state.json"),
        }
    }
}

impl CheckpointStore for FileCheckpointStore {
    fn load(&self) -> ImportFile {
        std::fs::read_to_string(&self.path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    fn save(&self, file: &ImportFile) -> std::result::Result<(), String> {
        let json = serde_json::to_vec_pretty(file).map_err(|error| error.to_string())?;
        write_atomic(&self.path, &json).map_err(|error| error.to_string())
    }
}

struct TokioPacer;

#[async_trait::async_trait]
impl Pacer for TokioPacer {
    async fn sleep(&self, delay: Duration) {
        tokio::time::sleep(delay).await;
    }
}

fn read_file(workspace_dir: &Path) -> ImportFile {
    FileCheckpointStore::new(workspace_dir).load()
}

/// Whether the last import ended with its last batch not confirmed listed:
/// the layout migration then forgets by id, never erasing scopes whole.
pub(crate) fn listed_unconfirmed(workspace_dir: &Path) -> bool {
    read_file(workspace_dir).listed_unconfirmed
}

fn import_error(error: &ImportError) -> Error {
    Error::InvalidRequest(error.to_string())
}

fn importer(
    config: &Config,
    engine: Arc<dyn tinymemory_api::MemoryEngine>,
    consent: bool,
    paused: Option<PauseCheck>,
) -> Arc<Importer> {
    let mut importer = Importer::new(
        config.workspace_dir.clone(),
        engine,
        Arc::new(FileCheckpointStore::new(&config.workspace_dir)),
        Arc::new(TokioPacer),
        Arc::new(move || consent),
    )
    .with_describe(Arc::new(|error| error.to_string()));
    if let Some(paused) = paused {
        importer = importer.with_pause(paused);
    }
    Arc::new(importer)
}

/// `memory_import_scan`.
pub async fn scan(config: &Config) -> Result<ImportScanView> {
    let workspace_dir = config.workspace_dir.clone();
    let counts = tokio::task::spawn_blocking(move || {
        tinymemory_integrations::import::count_legacy(&workspace_dir)
    })
    .await
    .map_err(|error| Error::Engine(format!("import scan failed: {error}")))?;
    let counts = counts.map(|counts: LegacyImportCounts| ImportCounts {
        documents: counts.documents,
        conversations: counts.conversations,
        learnings: counts.learnings,
    });
    Ok(ImportScanView {
        found: counts.is_some(),
        counts,
    })
}

/// Whether an import blocks layout migration: a live import or one paused for
/// credits. A persisted `Running` phase without a run slot is interrupted, not
/// live work; the scheduler can resume it before migration starts.
#[must_use]
pub fn in_progress(config: &Config) -> bool {
    IMPORT.is_running(&config.workspace_dir) || read_file(&config.workspace_dir).paused_for_credits
}

/// Whether this workspace has an import task right now.
#[must_use]
pub(crate) fn is_running(workspace_dir: &Path) -> bool {
    IMPORT.is_running(&workspace_dir.to_path_buf())
}

/// `memory_import_status`.
#[must_use]
pub fn status(config: &Config) -> ImportState {
    let file = read_file(&config.workspace_dir);
    let mut state = file.state;
    if state.phase == ImportPhase::Running && !is_running(&config.workspace_dir) {
        state.phase = ImportPhase::Error;
        state.error = Some(state.error.unwrap_or_else(|| {
            "the import was interrupted; it resumes on its own within a few minutes, or resume it now"
                .to_string()
        }));
    }
    state
}

/// The scheduler's pause, which includes being signed out.
pub(crate) fn scheduler_paused() -> bool {
    matches!(
        crate::platform::llm_gate::current_policy(),
        crate::platform::llm_gate::Policy::Paused { .. }
    )
}

/// Resumes an import the app quit in the middle of, if there is one, under
/// the scheduler's pause and current billing policy.
pub async fn resume_interrupted(ctx: &crate::core::runtime::DispatchCtx, config: &Config) -> bool {
    let file = read_file(&config.workspace_dir);
    let free = file.state.phase == ImportPhase::Error
        && file.paused_for_credits
        && super::billing::free_period_active(config).await;
    resume_interrupted_with(
        ctx,
        config,
        Arc::new(scheduler_paused),
        Arc::new(move |_: &Config| free),
    )
    .await
}

/// Resumes an interrupted import unless `paused` says background work is
/// paused. A failed automatic start is recorded through Native's checkpoint
/// helper so it cannot loop forever.
pub(crate) async fn resume_interrupted_with(
    ctx: &crate::core::runtime::DispatchCtx,
    config: &Config,
    paused: PauseCheck,
    billing: BillingCheck,
) -> bool {
    let store = FileCheckpointStore::new(&config.workspace_dir);
    let file = store.load();
    let plan = Importer::resume_plan(&file, billing(config), paused());
    let mode = match plan {
        ResumePlan::DoNotResume => return false,
        ResumePlan::ResumeImport => Mode::Import,
        ResumePlan::ResumeRetry => Mode::ResumeRetry,
    };
    let paused_for_batches = Some(paused);
    match launch(ctx, config, mode, true, paused_for_batches).await {
        Ok(state) => {
            tracing::info!(
                imported = state.imported,
                total = state.total,
                "[memory:import] resuming an interrupted import"
            );
            true
        }
        Err(error) => {
            tracing::warn!(
                code = error.code(),
                "[memory:import] interrupted import could not resume; stopped"
            );
            tinymemory_integrations::import::record_resume_failure(&store, &error.to_string());
            false
        }
    }
}

/// `memory_import_start`: requires `consent`, memory on, and a legacy store.
pub async fn start(
    ctx: &crate::core::runtime::DispatchCtx,
    config: &Config,
    consent: bool,
) -> Result<ImportState> {
    launch(ctx, config, Mode::Import, consent, None).await
}

/// `memory_import_retry_failed`: stores again the items a finished import
/// skipped because the engine refused them.
pub async fn retry_failed(
    ctx: &crate::core::runtime::DispatchCtx,
    config: &Config,
) -> Result<ImportState> {
    launch(ctx, config, Mode::Retry, true, None).await
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Import,
    Retry,
    ResumeRetry,
}

/// Claims the workspace, begins the Native run and transfers its slot guard
/// into the context-carrying worker future.
async fn launch(
    ctx: &crate::core::runtime::DispatchCtx,
    config: &Config,
    mode: Mode,
    consent: bool,
    paused: Option<PauseCheck>,
) -> Result<ImportState> {
    if mode == Mode::Import && !consent {
        return Err(import_error(&ImportError::ConsentRequired));
    }
    let bound = engine::resolve(config).engine()?;
    let workspace = config.workspace_dir.clone();
    let Some(guard) = run_slots::reserve(&IMPORT, &workspace, &[&BACKFILL, &MIGRATION]) else {
        if is_running(&workspace) {
            return Ok(status(config));
        }
        let reason = if MIGRATION.is_running(&workspace) {
            "memory is being organized; import once that finishes"
        } else {
            "a conversation backfill is running; import once it finishes"
        };
        return Err(Error::InvalidRequest(reason.into()));
    };
    let driver = importer(config, bound.engine, consent, paused);
    let begin = {
        let driver = driver.clone();
        ctx.spawn_blocking(move || {
            let begun = match mode {
                Mode::Import => driver.begin(),
                Mode::Retry => driver.begin_retry(),
                Mode::ResumeRetry => Ok(driver.begin_retry_resume()),
            };
            (guard, begun)
        })
        .await
    };
    let (guard, begun) = match begin {
        Ok((guard, Ok(begun))) => (guard, begun),
        Ok((_guard, Err(error))) => return Err(import_error(&error)),
        Err(error) => return Err(Error::Engine(format!("import start failed: {error}"))),
    };
    let state = begun.state.clone();
    tracing::info!(
        total = state.total,
        retry = mode != Mode::Import,
        "[memory:import] run started"
    );
    let config = config.clone();
    let task_ctx = ctx.clone();
    task_ctx.spawn(async move {
        {
            let _guard = guard;
            if mode == Mode::Import {
                driver.run(begun).await;
            } else {
                driver.run_retry(begun).await;
            }
        }
        // Import first, then organize. Migration's peer check sees the import
        // slot released before it attempts its own atomic reservation.
        let workspace_dir = config.workspace_dir.clone();
        if mode == Mode::Import && read_file(&workspace_dir).state.phase == ImportPhase::Done {
            let host = Arc::new(super::layout_migration::AppHost::new(config.clone()));
            let started = super::layout_migration::start(
                &task_ctx,
                config,
                host,
                super::layout_migration::Trigger::Manual { takeover: false },
                Arc::new(scheduler_paused),
            );
            tracing::info!(started, "[memory:import] import done; organizing next");
        }
    });
    Ok(state)
}

#[cfg(test)]
#[path = "import_tests.rs"]
mod tests;
