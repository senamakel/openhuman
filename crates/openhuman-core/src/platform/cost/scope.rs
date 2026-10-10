//! Building a [`UsageScope`] from what the recording task knows.
//!
//! Usage is recorded deep in the agent loop, where no caller passes thread or
//! agent ids down. They are already in scope as task-locals, though: the turn
//! origin (thread, channel or job, bound on the turn's `CoreContext`), the memory identity (the agent definition
//! acting) and the [`CoreContext`] (the embedded or SaaS user agent). The
//! recording site adds what only it knows — the provider, and the sub-agent
//! when a delegated child made the call.
//!
//! [`CoreContext`]: crate::core::runtime::CoreContext

use crate::agent::turn_origin::{AgentTurnOrigin, TrustedAutomationSource};

use super::types::UsageScope;

impl UsageScope {
    /// The scope of the calling task, with `provider` and the sub-agent (its
    /// definition and task id) supplied by the recording site.
    pub fn ambient(provider: Option<&str>, subagent: Option<(&str, &str)>) -> Self {
        let origin = crate::core::runtime::CoreContext::current_turn_origin();
        let (thread_id, origin) = describe_origin(origin.as_ref());
        let definition = crate::memory::scope::current().and_then(|identity| identity.agent_id);
        let session_agent = crate::core::runtime::current_tenant()
            .ok()
            .and_then(|tenant| tenant.agent);
        let (agent_id, subagent_task_id) = match subagent {
            Some((agent, task)) => (Some(agent.to_string()), Some(task.to_string())),
            None => (definition, None),
        };
        Self {
            thread_id,
            origin,
            agent_id,
            subagent_task_id,
            session_agent,
            provider: provider.filter(|p| !p.is_empty()).map(str::to_owned),
        }
    }
}

/// The thread a turn origin names and its short label.
pub(crate) fn describe_origin(
    origin: Option<&AgentTurnOrigin>,
) -> (Option<String>, Option<String>) {
    match origin {
        None | Some(AgentTurnOrigin::Unknown) => (None, None),
        Some(AgentTurnOrigin::WebChat { thread_id, .. }) => {
            (Some(thread_id.clone()), Some("web_chat".to_string()))
        }
        // A channel conversation's history key is its stable identity.
        Some(AgentTurnOrigin::ExternalChannel {
            channel,
            history_key,
            ..
        }) => (
            history_key.clone().filter(|key| !key.trim().is_empty()),
            Some(format!("channel:{channel}")),
        ),
        Some(AgentTurnOrigin::TrustedAutomation { source, .. }) => {
            let label = match source {
                TrustedAutomationSource::Cron => "cron",
                TrustedAutomationSource::Background => "background",
                TrustedAutomationSource::Workflow { .. } => "workflow",
            };
            (None, Some(label.to_string()))
        }
        Some(AgentTurnOrigin::Cli) => (None, Some("cli".to_string())),
        Some(AgentTurnOrigin::DirectChat) => (None, Some("direct_chat".to_string())),
    }
}

#[cfg(test)]
#[path = "scope_tests.rs"]
mod tests;
