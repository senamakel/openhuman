use anyhow::Result;
use tinyagents_runtime::{ResumeMode, SessionTurnRequest, TurnOptions};

use crate::agent::{
    message_convert::user_message_from_text, session_host::types::OpenHumanSessionHost,
    tinyagents::host::OpenHumanRunContext,
};

impl OpenHumanSessionHost {
    /// Dispatch a turn with authority supplied by its owning entry point.
    pub async fn turn_with_origin(
        &mut self,
        user_message: &str,
        origin: Option<&crate::security::turn_origin::AgentTurnOrigin>,
    ) -> Result<String> {
        self.turn_with_approval(user_message, origin, Default::default())
            .await
    }

    /// [`Self::turn_with_origin`] with the approval context of the turn: where
    /// its parked approvals surface, whether it is the flow copilot and which
    /// flow run it belongs to. It rides this turn's run context only, so it is
    /// never left behind on the session for a later turn to inherit.
    pub async fn turn_with_approval(
        &mut self,
        user_message: &str,
        origin: Option<&crate::security::turn_origin::AgentTurnOrigin>,
        approval: crate::security::approval::ApprovalTurnContext,
    ) -> Result<String> {
        // The turn's context is bound once, here, before the runtime session
        // snapshots the host environment: the one ambient read at the turn
        // boundary, for a host that did not hand a context over.
        if self.host_env.core.is_none() {
            self.host_env.core = crate::core::runtime::DispatchCtx::ambient().downgrade();
        }
        self.ensure_runtime_session()?;
        let staged = crate::attachments::stage_turn(
            user_message,
            self.runtime_config.as_deref(),
            self.workspace_descriptor.as_ref(),
            self.thread_id.as_deref(),
            origin,
        )
        .await?;
        let mut context = OpenHumanRunContext::new().with_host_env(&self.host_env);
        context.untrusted_input_turn = self.untrusted_input;
        context.origin = origin.cloned();
        context.approval = approval;
        context.progress = self.on_progress.clone();
        context.thread_id = self.thread_id.clone();
        context.task_recency_window = self.task_recency_window;
        context.autonomous_iter_cap = self.autonomous_iter_cap;
        context.workspace = self.workspace_descriptor.clone();
        // Sub-agents of this turn write their transcripts to this session's
        // store, under the key it resolved for its own.
        context.transcripts = Some(self.session_locator());
        // The web backstop's deadline (`host_env.turn_deadline`), when this
        // turn runs under one, is already on the context: the harness winds
        // down before it instead of being dropped by it.
        let cancellation = context.cancellation.clone();
        let root_config = context.root_run_config("openhuman-session");
        let mut options = TurnOptions {
            request_id: origin.and_then(|origin| match origin {
                crate::security::turn_origin::AgentTurnOrigin::WebChat { request_id, .. } => {
                    request_id.clone()
                }
                _ => None,
            }),
            thread_id: self.thread_id.clone(),
            stream: self.on_progress.is_some(),
            session: self.session.clone(),
            resume: self.turn_resume_mode(),
            cancellation,
            run_context: context.into_tinyagents(root_config),
            controls: Default::default(),
        };
        // The runtime's neutral `RuntimeError::Driver(String)` channel cannot
        // carry OpenHuman's typed dispatch refusal. Keep this per-turn sidecar
        // so the driver can restore the type after recording partial usage.
        let session_sidecar = options.run_context.data.session_sidecar.clone();
        // Load persisted state before the normal turn lifecycle so the host
        // can read native recorded declarations for both bound sessions and
        // cold, unthreaded latest-agent resumes.
        if matches!(
            options.resume,
            ResumeMode::Session | ResumeMode::LatestForAgent
        ) && self
            .runtime_session
            .as_ref()
            .is_some_and(|session| session.history().is_empty())
        {
            let runtime = self
                .runtime_session
                .as_mut()
                .expect("runtime session initialized");
            let resumed = runtime
                .resume(&options)
                .await
                .map_err(|error| anyhow::anyhow!(error.to_string()))?;
            if resumed.loaded {
                let recorded_tools = runtime.recorded_tools().cloned();
                if let Some(prelude) = self
                    .runtime_state
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .prelude
                    .clone()
                {
                    prelude.adopt_recorded_tools(recorded_tools.as_ref());
                }
            }
        }
        // Persisted declarations are historical facts, while the hook still
        // supplies the only executable sources and current policy.
        options.run_context.data.recorded_tool_names = std::sync::Arc::new(
            self.runtime_session
                .as_ref()
                .and_then(|runtime| runtime.recorded_tools())
                .map(|snapshot| {
                    snapshot
                        .specs()
                        .iter()
                        .map(|spec| spec.name.clone())
                        .collect()
                })
                .unwrap_or_default(),
        );
        let agent_id = self.agent_definition_id.clone();
        // The turn acts on memory as this agent, under whatever team the
        // caller already set on the run (a team member's turn).
        options.run_context.data.memory_identity = Some(
            options
                .run_context
                .data
                .memory_identity
                .clone()
                .unwrap_or_default()
                .child(&agent_id),
        );
        let turn = self
            .runtime_session
            .as_mut()
            .expect("runtime session initialized")
            .turn(
                SessionTurnRequest::new(user_message_from_text(staged.as_str())),
                options,
            );
        let outcome = match Box::pin(turn).await {
            Ok(outcome) => outcome,
            Err(error) => {
                return Err(runtime_turn_error(error, &session_sidecar));
            }
        };
        Ok(outcome.output.unwrap_or_default())
    }
}

fn runtime_turn_error(
    error: tinyagents_runtime::RuntimeError,
    sidecar: &std::sync::Arc<
        std::sync::Mutex<crate::agent::tinyagents::host::run_context::SessionTurnSidecar>,
    >,
) -> anyhow::Error {
    if matches!(error, tinyagents_runtime::RuntimeError::Driver(_)) {
        let refusal = sidecar
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .dispatch_refusal
            .take();
        if let Some(refusal) = refusal {
            return anyhow::Error::new(refusal);
        }
    }
    anyhow::anyhow!(error.to_string())
}

/// Applies the pending one-turn overrides to this turn's resume mode.
pub(in crate::agent::session_host) fn begin_turn_resume(
    state: &mut super::OpenHumanSessionState,
    resume: &mut ResumeMode,
) {
    let overrides = std::mem::take(&mut state.pending_turn_overrides);
    if overrides.suppress_transcript_autoload {
        *resume = ResumeMode::Never;
    }
    state.active_turn_overrides = overrides;
}

#[cfg(test)]
#[path = "runtime_session_turn_tests.rs"]
mod tests;
