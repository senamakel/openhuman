#[allow(clippy::module_inception)]
pub mod agent;
pub mod agents;
pub mod classifier;
pub mod context_pipeline;
pub mod dispatcher;
pub mod error;
pub mod harness;
pub mod hooks;
pub mod host_runtime;
pub mod identity;
pub mod memory_loader;
pub mod multimodal;
pub mod prompt;
mod schemas;
pub mod traits;
pub use schemas::{
    all_controller_schemas as all_agent_controller_schemas,
    all_registered_controllers as all_agent_registered_controllers,
};

#[cfg(test)]
mod tests;

#[allow(unused_imports)]
pub use agent::{Agent, AgentBuilder};
#[allow(unused_imports)]
pub use harness::{process_message, run};
