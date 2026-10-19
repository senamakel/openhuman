//! Deletions that must reach the engine, whenever it is reachable.
//!
//! Deleting a chat thread, forgetting a channel (one thread deletion per
//! thread it owned) or removing a source asks the engine to delete for good. When memory is off
//! (signed out, no credential) the engine cannot be reached, and when it
//! fails the deletion has not happened. Either way the deletion is recorded
//! here, in `<workspace>/memory/pending_deletions.json`, and [`drain`] runs
//! it again on the next sign-in ([`super::bus`], on `CredentialChanged`) and
//! on every background tick, until it succeeds.
//!
//! Every deletion is hard: a forget by filter removes the matched events by
//! `memory_ids` with an explicit `redact_events` cascade.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};

use async_trait::async_trait;
use tinymemory_tools::deletion::{Backoff, DeletionExecutor, DeletionQueue, Drain};
use tinymemory_tools::list_store::{ListStore, StoreError};
use tinymemory_tools::ops::MemoryOps;

pub use tinymemory_tools::deletion::PendingDeletion;

use crate::config::Config;

use super::engine;
use tinymemory_api::Error;

fn path(workspace_dir: &Path) -> PathBuf {
    workspace_dir.join("memory").join("pending_deletions.json")
}

/// `<workspace>/memory/pending_deletions.json`: owner-only, written through a
/// temporary file and a rename so a stop mid-write never loses the queue. An
/// unparsable file is set aside; an empty queue removes the file.
struct FileStore {
    file: PathBuf,
}

impl ListStore<PendingDeletion> for FileStore {
    fn load(&self) -> std::result::Result<Vec<PendingDeletion>, StoreError> {
        let Ok(text) = std::fs::read_to_string(&self.file) else {
            return Ok(Vec::new());
        };
        Ok(serde_json::from_str(&text).unwrap_or_else(|error| {
            tracing::warn!(%error, "[memory:deletion] pending deletions unparsable; set aside");
            if let Err(error) =
                std::fs::rename(&self.file, self.file.with_extension("json.corrupt"))
            {
                tracing::warn!(%error, "[memory:deletion] could not set the file aside");
            }
            Vec::new()
        }))
    }

    fn save(&self, all: &[PendingDeletion]) -> std::result::Result<(), StoreError> {
        if all.is_empty() {
            return match std::fs::remove_file(&self.file) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(StoreError(error.to_string())),
            };
        }
        let temp = self.file.with_extension("json.tmp");
        self.file
            .parent()
            .map_or(Ok(()), super::files::create_private_dir_all)
            .and_then(|()| {
                let json = serde_json::to_vec_pretty(all).map_err(std::io::Error::other)?;
                super::files::write_private(&temp, &json)?;
                std::fs::rename(&temp, &self.file)
            })
            .map_err(|error| StoreError(error.to_string()))
    }
}

type Queue = DeletionQueue<FileStore>;

/// One queue per workspace, so the in-memory retry backoff and the drain lock
/// are shared by every trigger.
static QUEUES: LazyLock<Mutex<HashMap<PathBuf, Arc<Queue>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn queue(workspace_dir: &Path) -> Arc<Queue> {
    let mut queues = QUEUES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    queues
        .entry(workspace_dir.to_path_buf())
        .or_insert_with(|| {
            Arc::new(DeletionQueue::new(
                FileStore {
                    file: path(workspace_dir),
                },
                Backoff::default(),
            ))
        })
        .clone()
}

/// Records `deletion` to run on the next [`drain`]. A deletion already
/// queued is not queued twice.
pub fn enqueue(workspace_dir: &Path, deletion: PendingDeletion) {
    queue(workspace_dir).enqueue(deletion);
}

/// Every deletion still owed, oldest first.
#[must_use]
pub fn pending(workspace_dir: &Path) -> Vec<PendingDeletion> {
    queue(workspace_dir).pending()
}

/// Runs queued deletions against the engine.
struct Executor<'a> {
    config: &'a Config,
}

#[async_trait]
impl DeletionExecutor for Executor<'_> {
    fn is_reachable(&self) -> bool {
        engine::resolve(self.config).is_on()
    }

    async fn run(&self, deletion: &PendingDeletion) -> std::result::Result<usize, String> {
        let bound = engine::resolve(self.config)
            .engine()
            .map_err(|error| error.code().to_string())?;
        // Pending source/thread filters keep TinyMemory's ordinary root-tree
        // default: all ordinary scopes, but never a service sandbox. Do not
        // narrow them to one authenticated profile root.
        let ops = MemoryOps::unconfined();
        ops.forget_or_enqueue(Some(bound.engine.as_ref()), deletion.clone(), |deletion| {
            enqueue(&self.config.workspace_dir, deletion);
        })
        .await
        .map_err(|error| error.code().to_string())
    }
}

/// Runs every queued deletion now (a sign-in), settling each one that
/// succeeds. Returns how many settled.
pub async fn drain(config: &Config) -> usize {
    queue(&config.workspace_dir)
        .drain(&Executor { config }, Drain::Force, chrono::Utc::now())
        .await
}

/// The background tick: like [`drain`], but a deletion that failed recently
/// waits out its backoff.
pub async fn drain_due(config: &Config) -> usize {
    queue(&config.workspace_dir)
        .drain(&Executor { config }, Drain::Due, chrono::Utc::now())
        .await
}

/// Forgets a deleted thread's conversation memory, for good. Memory off or
/// a failed forget queues the deletion for the next [`drain`] rather than
/// failing the caller: the thread itself is already gone. Returns how many
/// items went now.
pub async fn forget_thread(config: &Config, thread_id: &str) -> usize {
    if thread_id.trim().is_empty() {
        return 0;
    }
    let bound = match engine::resolve(config).engine() {
        Ok(bound) => Some(bound),
        Err(Error::Off(_)) => None,
        Err(error) => {
            tracing::warn!(
                code = error.code(),
                "[memory:deletion] forgetting a deleted thread failed; queued"
            );
            enqueue(
                &config.workspace_dir,
                PendingDeletion::Thread {
                    thread_id: thread_id.to_string(),
                },
            );
            return 0;
        }
    };
    // Match queued deletion's ordinary root-tree filter, including when the
    // profile root sits above other ordinary namespaces.
    let ops = MemoryOps::unconfined();
    let engine = bound.as_ref().map(|bound| bound.engine.as_ref());
    match ops
        .forget_thread(engine, thread_id, |deletion| {
            enqueue(&config.workspace_dir, deletion);
        })
        .await
    {
        Ok(forgotten) => forgotten,
        Err(error) => {
            tracing::warn!(
                code = error.code(),
                "[memory:deletion] forgetting a deleted thread failed; queued"
            );
            0
        }
    }
}

#[cfg(test)]
#[path = "deletion_tests.rs"]
mod tests;
