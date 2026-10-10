//! Durable sub-agent sessions on the `tinystoragedrivers` document port.
//!
//! Used instead of `<workspace>/.openhuman/subagent_sessions.json` when the
//! host configured a storage backend ([`crate::storage`]); `store.rs` picks it
//! per call, so the sessions live under the acting agent's storage scope
//! (`local` on a single-user host).
//!
//! # Layout
//!
//! | Collection | Document id | Holds |
//! | --- | --- | --- |
//! | `subagent_sessions` | the session id | one [`DurableSubagentSession`] plus `created_at` for ordering |
//!
//! The file store loads and saves the whole list, last writer wins. `save`
//! keeps that contract for the callers in `ops.rs`, but writes each session as
//! its own document, so a session's history stays well under any document-size
//! limit and two processes only collide on a session they both changed. `save`
//! never deletes: `ops.rs` only replaces a session (remove then re-add the same
//! id) and never drops one, and a delete sweep over a stale snapshot would
//! remove sessions another process added meanwhile.

use anyhow::Result;
use serde_json::Value;
use tinystoragedrivers::{CollectionSpec, Precondition, Query, Sort};

use super::types::DurableSubagentSession;
use crate::storage::documents::Repo;
use crate::storage::DocumentStoreExt;

const SESSIONS: &str = "subagent_sessions";
const DOMAIN: &str = "subagent_sessions::store";

fn collections() -> Vec<CollectionSpec> {
    vec![CollectionSpec::new(SESSIONS)]
}

/// The document store for this call, when the host configured one.
pub(super) fn current() -> Result<Option<Docs>> {
    #[cfg(test)]
    if let Some(docs) = TEST_OVERRIDE.with(|slot| slot.borrow().clone()) {
        return Ok(Some(docs));
    }
    Ok(Repo::current(DOMAIN, collections)?.map(Docs))
}

/// Runs `f` with `docs` standing in for the installed backend, on this thread
/// only, so tests exercise the dispatch without the process-wide slot.
#[cfg(test)]
pub(super) fn with_override<T>(docs: Docs, f: impl FnOnce() -> T) -> T {
    TEST_OVERRIDE.with(|slot| *slot.borrow_mut() = Some(docs));
    let out = f();
    TEST_OVERRIDE.with(|slot| *slot.borrow_mut() = None);
    out
}

#[cfg(test)]
thread_local! {
    static TEST_OVERRIDE: std::cell::RefCell<Option<Docs>> = const { std::cell::RefCell::new(None) };
}

/// The session list over one scoped document handle.
#[derive(Clone)]
pub(super) struct Docs(Repo);

impl Docs {
    #[cfg(test)]
    pub(super) fn over(scoped: &crate::storage::ScopedStorage) -> Self {
        Self(Repo::over(scoped, DOMAIN, collections))
    }

    pub(super) fn load(&self) -> Result<Vec<DurableSubagentSession>> {
        let stored = self.0.run(|docs| async move {
            docs.query_all(
                SESSIONS,
                &Query::all()
                    .sort(Sort::asc("created_at"))
                    .sort(Sort::asc("_id")),
            )
            .await
        })?;
        stored
            .into_iter()
            .map(|item| Ok(serde_json::from_value(item.doc)?))
            .collect()
    }

    pub(super) fn save(&self, sessions: &[DurableSubagentSession]) -> Result<()> {
        let mut writes = Vec::with_capacity(sessions.len());
        for session in sessions {
            let mut doc = serde_json::to_value(session)?;
            doc["created_at"] = Value::from(session.created_at.as_str());
            writes.push((session.subagent_session_id.clone(), doc));
        }
        log::debug!("[subagent_sessions] document save count={}", writes.len());
        self.0.run(|docs| async move {
            for (id, doc) in writes {
                docs.put(SESSIONS, &id, doc, Precondition::None).await?;
            }
            Ok(())
        })
    }
}

#[cfg(test)]
#[path = "store_documents_tests.rs"]
mod tests;
