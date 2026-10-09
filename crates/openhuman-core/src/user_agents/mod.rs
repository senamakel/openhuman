//! User agents: in SaaS mode, each user is served as one agent.
//!
//! The gateway authenticates users; this domain turns a gateway user id into
//! the agent that serves that user ([`UserAgentId`]), lays out the agent's
//! private state ([`layout`]), forces the config it runs with
//! ([`layout::agent_config`]), and keeps the open agents of the process
//! ([`AgentHost`]). The operator plane provisions and inspects them through
//! the `user_agents.*` controllers ([`schemas`]).
//!
//! The isolation boundary is the agent's own [`CoreContext`]: its config,
//! workspace and `session_agent`. Work for one user runs under that context,
//! which is what the config loader, the session store and the per-thread
//! caches key on.
//!
//! [`CoreContext`]: crate::core::runtime::CoreContext

pub mod background;
pub mod credentials;
pub mod gateway;
pub mod host;
pub mod layout;
pub mod ops;
pub mod schemas;
pub mod surface;
pub mod types;

pub use host::{current, AgentHost, UserAgentState};
pub use schemas::{all_user_agents_controller_schemas, all_user_agents_registered_controllers};
pub use types::{UserAgentId, UserAgentSummary};
