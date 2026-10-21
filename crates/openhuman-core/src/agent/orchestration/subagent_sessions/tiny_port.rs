//! Adapter from TinyAgents' durable subagent-session port to OpenHuman storage.

use std::path::PathBuf;

use async_trait::async_trait;
use tinyagents_orchestration::subagent::{
    DurableSubagentSession as TinySession, DurableSubagentStatus as TinyStatus,
    ReuseDecision as TinyReuseDecision, SubagentError, SubagentSessionSelector as TinySelector,
    SubagentSessionStore as TinySessionStore, SubagentSessionUpsert as TinyUpsert,
};

use super::types::{DurableSubagentSession, DurableSubagentStatus, SubagentSessionSelector};
use super::{SubagentSessionStore, SubagentSessionUpsert};

/// Routes generic Jobs/Message session operations through OpenHuman's existing
/// workspace-scoped session store. Legacy rows acquire a lifecycle key only
/// from their validated pause checkpoint; the public task id is never enough.
#[derive(Clone)]
pub(crate) struct OpenHumanTinySubagentSessionStore {
    store: SubagentSessionStore,
    checkpoint_dir: PathBuf,
    parent_session: String,
    parent_thread_id: Option<String>,
    jobs: Option<tinyagents_orchestration::subagent::SubAgentJobRegistry>,
}

impl OpenHumanTinySubagentSessionStore {
    pub(crate) fn new(
        workspace_dir: PathBuf,
        parent_session: impl Into<String>,
        parent_thread_id: Option<String>,
    ) -> Self {
        Self {
            checkpoint_dir: workspace_dir.join(".openhuman/subagent_checkpoints"),
            store: SubagentSessionStore::new(workspace_dir),
            parent_session: parent_session.into(),
            parent_thread_id,
            jobs: None,
        }
    }

    pub(crate) fn with_jobs(
        mut self,
        jobs: tinyagents_orchestration::subagent::SubAgentJobRegistry,
    ) -> Self {
        self.jobs = Some(jobs);
        self
    }

    fn is_owned(&self, row: &DurableSubagentSession) -> bool {
        row.parent_session == self.parent_session && row.parent_thread_id == self.parent_thread_id
    }

    fn convert(&self, row: DurableSubagentSession) -> Result<TinySession, SubagentError> {
        let lifecycle_key = match (row.lifecycle_key.clone(), row.current_task_id.as_deref()) {
            (Some(key), Some(task_id))
                if key.task_id == task_id
                    && key.thread_id.as_deref() == row.parent_thread_id.as_deref() =>
            {
                Some(key)
            }
            (Some(_), _) => None,
            (None, Some(task_id)) => super::resolve_lifecycle_key(
                &self.store,
                &row.subagent_session_id,
                &row.parent_session,
                row.parent_thread_id.as_deref(),
                &self.checkpoint_dir,
            )
            .ok()
            .filter(|key| key.task_id == task_id),
            // Keep a pre-lifecycle row visible to list/query. Its missing key
            // is not reconstructed from the raw task id, and continuation by
            // session id remains fail-closed in TinyAgents.
            (None, None) => None,
        };
        Ok(TinySession {
            subagent_session_id: row.subagent_session_id,
            parent_session: row.parent_session,
            parent_thread_id: row.parent_thread_id,
            worker_thread_id: row.worker_thread_id,
            agent_id: row.agent_id,
            display_name: row.display_name,
            model: row.model,
            sandbox_mode: row.sandbox_mode,
            action_root: row.action_root,
            task_key: row.task_key,
            lifecycle_key,
            task_title: row.task_title,
            current_task_id: row.current_task_id,
            status: match row.status {
                DurableSubagentStatus::Running => TinyStatus::Running,
                DurableSubagentStatus::Idle => TinyStatus::Idle,
                DurableSubagentStatus::AwaitingUser => TinyStatus::AwaitingUser,
                DurableSubagentStatus::Failed => TinyStatus::Failed,
                DurableSubagentStatus::Closed => TinyStatus::Closed,
            },
            reusable: row.reusable,
            latest_history: row.latest_history.map(|history| {
                history
                    .iter()
                    .map(tinyagents_runtime::transcript::transcript_to_message)
                    .collect()
            }),
            latest_error: row.latest_error,
            created_at: row.created_at,
            updated_at: row.updated_at,
            last_used_at: row.last_used_at,
        })
    }

    fn selector(selector: TinySelector) -> SubagentSessionSelector {
        SubagentSessionSelector {
            parent_session: selector.parent_session,
            parent_thread_id: selector.parent_thread_id,
            agent_id: selector.agent_id,
            model: selector.model,
            sandbox_mode: selector.sandbox_mode,
            action_root: selector.action_root,
            task_key: selector.task_key,
        }
    }
}

#[async_trait]
impl TinySessionStore for OpenHumanTinySubagentSessionStore {
    async fn list_for_parent(
        &self,
        parent_session: &str,
        parent_thread_id: Option<&str>,
    ) -> Result<Vec<TinySession>, SubagentError> {
        if parent_session != self.parent_session {
            return Ok(Vec::new());
        }
        if parent_thread_id
            .is_some_and(|requested| self.parent_thread_id.as_deref() != Some(requested))
        {
            return Ok(Vec::new());
        }
        super::list_for_parent(
            &self.store,
            parent_session,
            self.parent_thread_id.as_deref(),
        )
        .map_err(SubagentError::Persistence)?
        .into_iter()
        .filter(|row| self.is_owned(row))
        .map(|row| self.convert(row))
        .collect()
    }

    async fn get(&self, id: &str) -> Result<Option<TinySession>, SubagentError> {
        let mut owned_rows = self
            .store
            .load()
            .map_err(SubagentError::Persistence)?
            .into_iter()
            // Filter the authenticated parent and thread before matching either
            // public identifier. Raw task IDs can collide across owners, so a
            // foreign row must never shadow a matching owned row.
            .filter(|row| self.is_owned(row))
            .collect::<Vec<_>>();

        // Product callers historically pass the durable session ID, while
        // accepted spawn results also expose `current_task_id`. Prefer the
        // unambiguous session key when both happen to match.
        let row = if let Some(index) = owned_rows
            .iter()
            .position(|row| row.subagent_session_id == id)
        {
            Some(owned_rows.remove(index))
        } else {
            let mut matches = owned_rows
                .into_iter()
                .filter(|row| row.current_task_id.as_deref() == Some(id));
            let row = matches.next();
            if matches.next().is_some() {
                return Err(SubagentError::InvalidRequest(
                    "subagent task id is ambiguous within the authenticated owner scope".into(),
                ));
            }
            row
        };

        row.map(|row| self.convert(row)).transpose()
    }

    async fn upsert_running(
        &self,
        upsert: TinyUpsert,
    ) -> Result<(TinySession, TinyReuseDecision), SubagentError> {
        if upsert.selector.parent_session != self.parent_session
            || upsert.selector.parent_thread_id != self.parent_thread_id
            || upsert.lifecycle_key.thread_id != self.parent_thread_id
            || upsert.lifecycle_key.task_id != upsert.task_id
        {
            return Err(SubagentError::InvalidRequest(
                "subagent session upsert is outside its authenticated owner scope".into(),
            ));
        }
        let selector = Self::selector(upsert.selector);
        let reuse = if upsert.force_fresh {
            None
        } else {
            super::find_reusable(&self.store, &selector).map_err(SubagentError::Persistence)?
        };
        let decision = if upsert.force_fresh {
            TinyReuseDecision::ForcedFresh
        } else if let Some(session) = reuse.as_ref() {
            if session.status == DurableSubagentStatus::Running {
                TinyReuseDecision::ReusedRunning
            } else {
                TinyReuseDecision::ReusedIdle
            }
        } else {
            TinyReuseDecision::SpawnedNew
        };
        let row = super::upsert_running(
            &self.store,
            SubagentSessionUpsert {
                selector,
                display_name: upsert.display_name,
                task_title: upsert.task_title,
                worker_thread_id: upsert.worker_thread_id,
                task_id: upsert.task_id,
                lifecycle_key: Some(upsert.lifecycle_key),
            },
            reuse.as_ref(),
        )
        .map_err(SubagentError::Persistence)?;
        Ok((self.convert(row)?, decision))
    }

    async fn close(&self, id: &str) -> Result<bool, SubagentError> {
        let session = self
            .store
            .load()
            .map_err(SubagentError::Persistence)?
            .into_iter()
            .find(|session| session.subagent_session_id == id && self.is_owned(session));
        let Some(session) = session else {
            return Ok(false);
        };
        if session.status == DurableSubagentStatus::Closed {
            return super::close_if_current(&self.store, &session)
                .map_err(SubagentError::Persistence);
        }
        if let Some(jobs) = &self.jobs {
            let _ = super::cancel_driver_job_for_observed_session(
                &self.store,
                &self.checkpoint_dir,
                jobs,
                &session,
                &self.parent_session,
                self.parent_thread_id.as_deref(),
            );
        }
        match super::close_if_current(&self.store, &session).map_err(SubagentError::Persistence)? {
            true => Ok(true),
            false => Err(SubagentError::InvalidRequest(
                "subagent session changed while it was being closed".into(),
            )),
        }
    }

    async fn mark_finished(
        &self,
        id: &str,
        task_id: &str,
        status: TinyStatus,
        history: Vec<tinyinference_llm::message::Message>,
    ) -> Result<bool, SubagentError> {
        if self.get(id).await?.is_none() {
            return Ok(false);
        }
        let status = match status {
            TinyStatus::Running => crate::agent::subagent_host::SubagentRunStatus::Incomplete {
                reason: "still running".into(),
                failure_cause: None,
            },
            TinyStatus::Idle => crate::agent::subagent_host::SubagentRunStatus::Completed,
            TinyStatus::AwaitingUser => {
                crate::agent::subagent_host::SubagentRunStatus::AwaitingUser {
                    question: history
                        .last()
                        .map(tinyinference_llm::message::Message::text)
                        .unwrap_or_default(),
                    options: None,
                    checkpoint: Some(self.checkpoint_dir.clone()),
                }
            }
            TinyStatus::Failed => crate::agent::subagent_host::SubagentRunStatus::Cancelled,
            TinyStatus::Closed => return self.close(id).await,
        };
        let history = history
            .iter()
            .filter_map(tinyagents_runtime::transcript::message_to_native_transcript)
            .collect();
        super::mark_finished(&self.store, id, task_id, &status, history)
            .map_err(SubagentError::Persistence)?;
        Ok(true)
    }

    async fn mark_failed(
        &self,
        id: &str,
        task_id: &str,
        error: String,
    ) -> Result<bool, SubagentError> {
        if self.get(id).await?.is_none() {
            return Ok(false);
        }
        super::mark_failed(&self.store, id, task_id, error).map_err(SubagentError::Persistence)?;
        Ok(true)
    }
}

#[cfg(test)]
#[path = "tiny_port_tests.rs"]
mod tests;
