//! The detached-task ledger on the `tinystoragedrivers` document port.
//!
//! Used instead of `<workspace>/.openhuman/orchestration_tasks.jsonl` when the
//! host configured a storage backend ([`crate::storage`]); `task_ledger.rs`
//! picks it when it opens a workspace's store. The store lives under the
//! storage scope of the call that opened it (the acting agent, `local` on a
//! single-user host), and `task_ledger.rs` keys its cache by that scope.
//!
//! # Layout
//!
//! | Collection | Document id | Holds |
//! | --- | --- | --- |
//! | `orchestration_tasks` | task id | `record` (the latest [`OrchestrationTaskRecord`]) and `history` (every transition, oldest first) |
//!
//! The transition rules are TinyAgents' (`InMemoryTaskStore` is the state
//! machine); this type only replays the documents on open and writes the
//! changed task's document after each transition, as `JsonlTaskStore` appends
//! a line. A task id belongs to one process (it is a detached run's id), so
//! the document is replaced outright rather than compare-and-swapped.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use anyhow::Result as AnyResult;
use serde_json::{json, Value};
use tinyagents_harness::error::{Result, TinyAgentsError};
use tinyagents_harness::ids::TaskId;
use tinyagents_tasks::{
    InMemoryTaskStore, OrchestrationControlOutcome, OrchestrationTaskFilter,
    OrchestrationTaskRecord, OrchestrationTaskResult, OrchestrationTaskSpec, TaskStore,
};
use tinystoragedrivers::{CollectionSpec, Precondition, Query};

use crate::storage::documents::Repo;
use crate::storage::DocumentStoreExt;

const TASKS: &str = "orchestration_tasks";
const DOMAIN: &str = "running_subagents::task_ledger";

fn collections() -> Vec<CollectionSpec> {
    vec![CollectionSpec::new(TASKS)]
}

/// The repo for this call, when the host configured a backend.
pub(super) fn current() -> AnyResult<Option<Repo>> {
    #[cfg(test)]
    if let Some(repo) = TEST_OVERRIDE.with(|slot| slot.borrow().clone()) {
        return Ok(Some(repo));
    }
    Repo::current(DOMAIN, collections)
}

/// Whether a test pinned a document store for this thread.
#[cfg(test)]
pub(super) fn overridden() -> bool {
    TEST_OVERRIDE.with(|slot| slot.borrow().is_some())
}

/// Runs `f` with `repo` standing in for the installed backend, on this thread
/// only, so tests exercise the dispatch without the process-wide slot.
#[cfg(test)]
pub(super) fn with_override<T>(repo: Repo, f: impl FnOnce() -> T) -> T {
    TEST_OVERRIDE.with(|slot| *slot.borrow_mut() = Some(repo));
    let out = f();
    TEST_OVERRIDE.with(|slot| *slot.borrow_mut() = None);
    out
}

#[cfg(test)]
thread_local! {
    static TEST_OVERRIDE: std::cell::RefCell<Option<Repo>> = const { std::cell::RefCell::new(None) };
}

/// A [`TaskStore`] that keeps every task as a document.
pub(super) struct DocumentTaskStore {
    inner: InMemoryTaskStore,
    repo: Repo,
    history: Mutex<HashMap<TaskId, Vec<OrchestrationTaskRecord>>>,
}

fn graph_error(what: &str, error: impl std::fmt::Display) -> TinyAgentsError {
    TinyAgentsError::Graph(format!("{what}: {error}"))
}

impl DocumentTaskStore {
    /// Opens the ledger over `repo`, replaying every stored task.
    pub(super) fn open(repo: Repo) -> Result<Self> {
        let stored = repo
            .run(|docs| async move { docs.query_all(TASKS, &Query::all()).await })
            .map_err(|error| graph_error("read task ledger", error))?;
        let mut history: HashMap<TaskId, Vec<OrchestrationTaskRecord>> = HashMap::new();
        let mut latest = Vec::new();
        for item in stored {
            let record: Option<OrchestrationTaskRecord> = item
                .doc
                .get("record")
                .and_then(|value| serde_json::from_value(value.clone()).ok());
            let Some(record) = record else {
                log::warn!(
                    "[running_subagents] skipping malformed task document id={}",
                    item.id
                );
                continue;
            };
            let timeline: Vec<OrchestrationTaskRecord> = item
                .doc
                .get("history")
                .and_then(|value| serde_json::from_value(value.clone()).ok())
                .unwrap_or_else(|| vec![record.clone()]);
            history.insert(record.spec.task_id.clone(), timeline);
            latest.push(record);
        }
        log::debug!(
            "[running_subagents] opened document task ledger tasks={}",
            latest.len()
        );
        Ok(Self {
            inner: InMemoryTaskStore::from_records(latest),
            repo,
            history: Mutex::new(history),
        })
    }

    fn persist(&self, record: &OrchestrationTaskRecord) -> Result<()> {
        let task_id = record.spec.task_id.clone();
        let timeline = {
            let mut history = self
                .history
                .lock()
                .map_err(|_| graph_error("task ledger", "history lock poisoned"))?;
            let timeline = history.entry(task_id.clone()).or_default();
            timeline.push(record.clone());
            timeline.clone()
        };
        let doc: Value = json!({
            "record": record,
            "history": timeline,
        });
        let id = task_id.as_str().to_string();
        self.repo
            .run(|docs| async move {
                docs.put(TASKS, &id, doc, Precondition::None)
                    .await
                    .map(|_| ())
            })
            .map_err(|error| graph_error("write task ledger", error))
    }

    fn persist_current(&self, task_id: &TaskId) -> Result<()> {
        if let Some(record) = self.inner.get(task_id) {
            self.persist(&record)?;
        }
        Ok(())
    }
}

impl TaskStore for DocumentTaskStore {
    fn insert(&self, spec: OrchestrationTaskSpec) -> Result<OrchestrationTaskRecord> {
        let record = self.inner.insert(spec)?;
        self.persist(&record)?;
        Ok(record)
    }

    fn get(&self, task_id: &TaskId) -> Option<OrchestrationTaskRecord> {
        self.inner.get(task_id)
    }

    fn list(&self, filter: OrchestrationTaskFilter) -> Vec<OrchestrationTaskRecord> {
        self.inner.list(filter)
    }

    fn history(&self, task_id: &TaskId) -> Vec<OrchestrationTaskRecord> {
        self.history
            .lock()
            .ok()
            .and_then(|history| history.get(task_id).cloned())
            .unwrap_or_default()
    }

    fn mark_running(&self, task_id: &TaskId) -> Result<OrchestrationTaskRecord> {
        let record = self.inner.mark_running(task_id)?;
        self.persist(&record)?;
        Ok(record)
    }

    fn mark_awaiting(&self, task_id: &TaskId) -> Result<OrchestrationTaskRecord> {
        let record = self.inner.mark_awaiting(task_id)?;
        self.persist(&record)?;
        Ok(record)
    }

    fn mark_awaiting_with_question(
        &self,
        task_id: &TaskId,
        question: String,
    ) -> Result<OrchestrationTaskRecord> {
        let record = self.inner.mark_awaiting_with_question(task_id, question)?;
        self.persist(&record)?;
        Ok(record)
    }

    fn complete(
        &self,
        task_id: &TaskId,
        result: OrchestrationTaskResult,
    ) -> Result<OrchestrationTaskRecord> {
        let record = self.inner.complete(task_id, result)?;
        self.persist(&record)?;
        Ok(record)
    }

    fn fail(&self, task_id: &TaskId, error: String) -> Result<OrchestrationTaskRecord> {
        let record = self.inner.fail(task_id, error)?;
        self.persist(&record)?;
        Ok(record)
    }

    fn timeout(&self, task_id: &TaskId, error: String) -> Result<OrchestrationTaskRecord> {
        let record = self.inner.timeout(task_id, error)?;
        self.persist(&record)?;
        Ok(record)
    }

    fn request_cancel(&self, task_id: &TaskId) -> Result<OrchestrationControlOutcome> {
        let outcome = self.inner.request_cancel(task_id)?;
        self.persist_current(task_id)?;
        Ok(outcome)
    }

    fn mark_cancelled(&self, task_id: &TaskId) -> Result<OrchestrationTaskRecord> {
        let record = self.inner.mark_cancelled(task_id)?;
        self.persist(&record)?;
        Ok(record)
    }

    fn kill(&self, task_id: &TaskId) -> Result<OrchestrationControlOutcome> {
        let outcome = self.inner.kill(task_id)?;
        self.persist_current(task_id)?;
        Ok(outcome)
    }

    fn set_timeout_ms(&self, task_id: &TaskId, timeout_ms: u64) -> Result<OrchestrationTaskRecord> {
        let record = self.inner.set_timeout_ms(task_id, timeout_ms)?;
        self.persist(&record)?;
        Ok(record)
    }
}

/// Opens the document ledger for `repo`, or an in-memory one when the
/// documents cannot be read (degrading like the file ledger does).
pub(super) fn open_or_memory(repo: Repo) -> Arc<dyn TaskStore> {
    match DocumentTaskStore::open(repo) {
        Ok(store) => Arc::new(store),
        Err(error) => {
            log::warn!(
                "[running_subagents] document task ledger unavailable; using memory: {error}"
            );
            Arc::new(InMemoryTaskStore::new())
        }
    }
}

#[cfg(test)]
#[path = "task_ledger_documents_tests.rs"]
mod tests;
