//! Storing past chats: conversations from before turns were logged.
//!
//! The TinyMemory backfill driver owns planning, progress, batching and
//! idempotent writes. This host adapter supplies the conversation thread
//! source, the existing state-file path, consent, engine binding and task
//! lifetime.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use crate::core::runtime::DispatchCtx;
use crate::config::Config;
use crate::memory::engine;
use crate::memory::run_slots::{self, BACKFILL, IMPORT, MIGRATION};
use crate::threads::store::blocking as threads;
use tinymemory_api::{Error, Result};
use tinymemory_tools::backfill::{BackfillFile, Backfiller, CounterStore, ThreadMessage, ThreadSource};
use tinymemory_tools::layout::identity::Identity;

pub use tinymemory_tools::backfill::{
    BackfillPhase, BackfillState, BackfillView, BACKFILL_TAG, MAIN_AGENT, PastTurn, pending_range,
    turns_of,
};

/// `memory_conversations_backfill_start` params.
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct BackfillStartParams {
    /// The caller agrees to upload past chats to the engine.
    #[serde(default)]
    pub consent: bool,
}

struct HostThreadSource {
    ctx: DispatchCtx,
    workspace_dir: PathBuf,
}

#[async_trait]
impl ThreadSource for HostThreadSource {
    async fn list_threads(&self) -> std::result::Result<Vec<String>, String> {
        threads::list_threads(self.ctx.clone(), self.workspace_dir.clone())
            .await
            .map(|threads| threads.into_iter().map(|thread| thread.id).collect())
            .map_err(|error| error.to_string())
    }

    async fn messages(&self, thread_id: &str) -> std::result::Result<Vec<ThreadMessage>, String> {
        threads::get_messages(
            self.ctx.clone(),
            self.workspace_dir.clone(),
            thread_id.to_string(),
        )
        .await
        .map(|messages| {
            messages
                .into_iter()
                .map(|message| {
                    ThreadMessage::new(message.sender, message.content, message.created_at)
                })
                .collect()
        })
        .map_err(|error| error.to_string())
    }
}

struct FileCounterStore {
    workspace_dir: PathBuf,
}

impl FileCounterStore {
    fn path(&self) -> PathBuf {
        self.workspace_dir
            .join("memory")
            .join("conversations_backfill.json")
    }
}

impl CounterStore for FileCounterStore {
    fn load(&self) -> BackfillFile {
        std::fs::read_to_string(self.path())
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    fn save(&self, file: &BackfillFile) -> std::result::Result<(), String> {
        let bytes = serde_json::to_vec_pretty(file).map_err(|error| error.to_string())?;
        crate::security::secret_storage::file_store::write_atomic(&self.path(), &bytes)
            .map_err(|error| error.to_string())
    }
}

fn driver(
    ctx: DispatchCtx,
    config: &Config,
    engine: Arc<dyn tinymemory_api::MemoryEngine>,
) -> Result<Backfiller> {
    let identity = Identity::agent(MAIN_AGENT).resolve(&crate::memory::scope::identity_pins(config));
    Ok(Backfiller::new(
        engine,
        identity.layout,
        identity.agent_id,
        Arc::new(HostThreadSource {
            ctx,
            workspace_dir: config.workspace_dir.clone(),
        }),
        Arc::new(FileCounterStore {
            workspace_dir: config.workspace_dir.clone(),
        }),
    ))
}

/// `memory_conversations_backfill_status`.
pub async fn status(ctx: &DispatchCtx, config: &Config) -> Result<BackfillView> {
    let bound = engine::resolve(config).engine()?;
    driver(ctx.clone(), config, bound.engine)?
        .status(BACKFILL.is_running(&config.workspace_dir))
        .await
}

/// `memory_conversations_backfill_start`: requires `consent` and memory on.
pub async fn start(
    ctx: &DispatchCtx,
    config: &Config,
    params: BackfillStartParams,
) -> Result<BackfillView> {
    if !params.consent {
        return Err(Error::InvalidRequest(
            "storing past chats uploads them to the selected engine; pass consent: true".into(),
        ));
    }
    let bound = engine::resolve(config).engine()?;
    let workspace = config.workspace_dir.clone();
    let Some(guard) = run_slots::reserve(&BACKFILL, &workspace, &[&IMPORT, &MIGRATION]) else {
        return status(ctx, config).await;
    };
    let backfiller = driver(ctx.clone(), config, bound.engine)?;
    let pending = backfiller.status(false).await?;
    let run = match backfiller.begin().await {
        Ok(run) => run,
        Err(error) => return Err(error),
    };
    let view = BackfillView {
        state: BackfillState {
            phase: BackfillPhase::Running,
            threads_total: pending.pending_threads,
            ..BackfillState::default()
        },
        pending_threads: pending.pending_threads,
        pending_turns: pending.pending_turns,
    };
    let task_ctx = ctx.clone();
    task_ctx.spawn(async move {
        let _guard = guard;
        let state = backfiller.run(run).await;
        if state.phase == BackfillPhase::Done {
            tracing::info!(items = state.items_stored, turns = state.turns_stored, "[memory:backfill] done");
        }
    });
    Ok(view)
}

/// Whether a backfill task currently owns this workspace.
pub(crate) fn is_running(workspace_dir: &Path) -> bool {
    BACKFILL.is_running(&workspace_dir.to_path_buf())
}
