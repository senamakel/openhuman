//! The runtime's agents, as the core's own drivers see them.
//!
//! The core's cron scheduler and workflow `agent` nodes only know an agent
//! id. [`RuntimeAgents`] answers for the ids alive on this runtime with
//! everything a session for that agent needs: its definition, its host tools
//! (spec belt plus attachments), its config with the provider model and route
//! applied, and its context. The runtime installs it on build and removes it
//! when the last owner of the core drops.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};

use openhuman_core::agent::host_agents::{HostAgent, HostAgentResolver};

use crate::agent::AgentInner;

/// The live agents of one runtime, by id.
pub(crate) type AgentMap = Mutex<HashMap<String, Weak<AgentInner>>>;

pub(crate) struct RuntimeAgents(pub(crate) Arc<AgentMap>);

impl HostAgentResolver for RuntimeAgents {
    fn resolve(&self, agent_id: &str) -> Option<HostAgent> {
        let inner = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(agent_id)?
            .upgrade()?;
        inner.host_agent()
    }
}

impl AgentInner {
    /// This agent as a core driver builds it. `None` when its provider route
    /// pairs a bearer with an endpoint a turn would refuse (plain `http:` to
    /// a remote host): the driver then fails to find the agent rather than
    /// sending the credential in the clear.
    pub(crate) fn host_agent(self: &Arc<Self>) -> Option<HostAgent> {
        if *self.lifecycle.removed().borrow() {
            return None;
        }
        let mut config = self.config.clone();
        if let Some(model) = self.provider.model_id() {
            config.default_model = Some(model.to_string());
        }
        if let Some(route) = self.provider.route() {
            if !crate::turn::is_safe_endpoint_for_bearer(&route.base_url) {
                log::warn!(
                    "[embed][agent] id={} has an insecure route; not offered to core drivers",
                    self.id
                );
                return None;
            }
            if let Some(route) = openhuman_core::config::schema::EphemeralRoute::from_params(
                Some(route.base_url.clone()),
                Some(route.api_key.clone()),
            )
            .map(|scoped| scoped.with_headers(route.headers.clone()))
            {
                openhuman_core::config::schema::ephemeral_route::apply(&mut config, route);
            }
        }
        Some(HostAgent {
            definition: self.definition.clone(),
            config,
            host_tools: self.composed_host_tools(),
            context: Arc::clone(&self.ctx),
            hooks: self.hooks.clone(),
        })
    }
}
