//! What the Computer settings page shows about the TinyComputer module: its
//! lifecycle state, the routes the host configured, and — when asked — what
//! the module itself reports through `Describe`.

use serde::Serialize;
use tinycomputer_bus::agent::{Capabilities, SurfaceAvailability};

use crate::config::Config;
use crate::modules::types::{ModuleState, ModuleStatus};

/// What the module reports about itself.
#[derive(Debug, Clone, Serialize)]
pub struct ComputerCapabilities {
    pub contract_version: (u32, u32),
    /// Whether this host can talk to that contract.
    pub compatible: bool,
    pub jev_configured: bool,
    pub planner_configured: bool,
    pub rescue_configured: bool,
    pub surfaces: Vec<SurfaceAvailability>,
}

impl From<Capabilities> for ComputerCapabilities {
    fn from(value: Capabilities) -> Self {
        Self {
            contract_version: value.contract_version,
            compatible: tinycomputer_bus::is_compatible(value.contract_version),
            jev_configured: value.jev_configured,
            planner_configured: value.planner_configured,
            rescue_configured: value.rescue_configured,
            surfaces: value.surfaces,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ComputerStatus {
    pub module: Option<ModuleStatus>,
    /// `[computer] decision_model`.
    pub decision_model: &'static str,
    /// Configured decision model route hint: `hosted`, `direct_openrouter`,
    /// `open_jev`, `sage`, or `unavailable`. It does not validate credentials.
    pub decision_route: &'static str,
    /// Configured planner route hint. It does not validate credentials.
    pub planner_route: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<ComputerCapabilities>,
    /// Why `Describe` could not be read, when it was asked for.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

fn planner_route(config: &Config) -> &'static str {
    crate::desktop::computer_config::configured_routes(config).planner
}

async fn describe(config: &Config, load: bool) -> Result<Capabilities, String> {
    let proxy = if load {
        crate::modules::desktop::proxy(config).await?
    } else {
        crate::modules::desktop::proxy_if_loaded().await?
    };
    describe_proxy(proxy).await
}

pub(crate) async fn describe_proxy(proxy: tinybus::Proxy) -> Result<Capabilities, String> {
    proxy
        .call(tinycomputer_bus::agent::names::methods::DESCRIBE, ())
        .await
        .map_err(|error| format!("TinyComputer Describe failed: {error}"))
}

/// Report the module. `load` asks the module itself (loading and refreshing
/// private configuration if needed); otherwise a ready module is queried
/// through the existing runtime without loading or reconfiguring it.
pub async fn status(config: &Config, load: bool) -> ComputerStatus {
    let module = || {
        crate::modules::ops::list(config)
            .into_iter()
            .find(|item| item.id == crate::modules::desktop::MODULE_ID)
    };
    let before = module();
    let ask = load
        || before
            .as_ref()
            .is_some_and(|m| m.state == ModuleState::Ready);
    let (capabilities, error) = if ask {
        match describe(config, load).await {
            Ok(capabilities) => (Some(capabilities.into()), None),
            Err(error) => {
                tracing::debug!(%error, "[computer] describe failed");
                (None, Some(error))
            }
        }
    } else {
        (None, None)
    };
    ComputerStatus {
        module: if ask { module() } else { before },
        decision_model: config.computer.decision_model.as_str(),
        decision_route: crate::desktop::computer_config::configured_routes(config).decision,
        planner_route: planner_route(config),
        capabilities,
        error,
    }
}

#[cfg(test)]
#[path = "computer_status_tests.rs"]
mod tests;
