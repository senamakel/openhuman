use std::collections::HashMap;

use super::*;

/// The schedulers of the live embedded agents, one per agent id.
#[derive(Default)]
pub(crate) struct LiveAgentSchedulers {
    by_agent: HashMap<String, JobDispatcher>,
}

impl LiveAgentSchedulers {
    /// Runs the due jobs of every live embedded agent, each under that agent's
    /// own context: its config, provider route, policy and job store. An agent
    /// that is not live has no context to run under — no host tools or prompt
    /// for its agent jobs — so its jobs stay dormant until it is instantiated
    /// again, including when a storage backend still records it
    /// (`crate::storage::agents`).
    pub(crate) async fn tick(&mut self) {
        let live = crate::core::runtime::AgentContextRegistry::live();
        // A scheduler whose agent is gone aborts the jobs it still owns.
        self.by_agent
            .retain(|id, _| live.iter().any(|(live_id, _)| live_id == id));
        for (agent_id, ctx) in live {
            let scheduler = &mut self.by_agent;
            let dispatch = crate::core::runtime::DispatchCtx::new(ctx.clone());
            crate::core::runtime::CoreContext::scope(ctx, async move {
                let config = match crate::config::ops::load_current_or_init(&dispatch).await {
                    Ok(config) => config,
                    Err(error) => {
                        tracing::debug!(agent = %agent_id, %error, "[cron:scheduler] agent config unavailable");
                        return;
                    }
                };
                let Some(security) = crate::security::live_policy::effective_in(&dispatch) else {
                    tracing::warn!(agent = %agent_id, "[cron:scheduler] agent has no policy; jobs skipped");
                    return;
                };
                let dispatcher = scheduler
                    .entry(agent_id.clone())
                    .or_insert_with(|| JobDispatcher::new(&dispatch, &config, &security));
                match dispatcher.tick(&config).await {
                    Ok(0) => {}
                    Ok(count) => {
                        tracing::debug!(agent = %agent_id, count, "[cron:scheduler] agent jobs dispatched");
                    }
                    Err(error) => {
                        tracing::warn!(agent = %agent_id, "[cron:scheduler] agent poll failed: {error}");
                    }
                }
            })
            .await;
        }
    }

    /// Wait for every dispatched agent job to finish.
    pub(crate) async fn drain(&mut self) {
        for dispatcher in self.by_agent.values_mut() {
            dispatcher.drain().await;
        }
    }
}

/// One pass over the live agents' due jobs, run to completion: what the
/// scheduler loop does after each operator tick, for hosts and end-to-end
/// tests that drive the agent pass without owning the loop's interval.
pub async fn run_live_agent_pass(max_concurrent: usize) {
    tracing::debug!(max_concurrent, "[cron:scheduler] live agent pass begin");
    let mut agents = LiveAgentSchedulers::default();
    agents.tick().await;
    agents.drain().await;
}
