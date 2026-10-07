//! `tools-system`: the tools that change the host process or its settings.

use std::sync::Arc;

use tinytools::Tool;

use crate::config::Config;
use crate::security::SecurityPolicy;
use crate::tools::{
    DaemonHostPrefsSetTool, ProxyConfigTool, ServiceInstallTool, ServiceRestartTool,
    ServiceShutdownTool, ServiceStartTool, ServiceStopTool, ServiceUninstallTool, UpdateApplyTool,
    WorkspaceUpdatePersonaTool,
};

/// `proxy_config`.
pub(crate) fn proxy_tools(
    config: &Arc<Config>,
    security: &Arc<SecurityPolicy>,
) -> Vec<Box<dyn Tool>> {
    vec![Box::new(ProxyConfigTool::new(
        Arc::clone(config),
        Arc::clone(security),
    ))]
}

/// `update_apply`.
pub(crate) fn update_apply_tools(security: &Arc<SecurityPolicy>) -> Vec<Box<dyn Tool>> {
    vec![Box::new(UpdateApplyTool::new(Arc::clone(security)))]
}

/// The service lifecycle mutators and `daemon_host_prefs_set`. They ship
/// default-OFF behind the `service_lifecycle` user toggle as well.
pub(crate) fn service_lifecycle_tools(config: &Arc<Config>) -> Vec<Box<dyn Tool>> {
    vec![
        Box::new(ServiceStartTool::new(Arc::clone(config))),
        Box::new(ServiceStopTool::new(Arc::clone(config))),
        Box::new(ServiceRestartTool),
        Box::new(ServiceShutdownTool),
        Box::new(ServiceInstallTool::new(Arc::clone(config))),
        Box::new(ServiceUninstallTool::new(Arc::clone(config))),
        Box::new(DaemonHostPrefsSetTool::new(Arc::clone(config))),
    ]
}

/// `workspace_update_persona`: rewrites the agent's own persona files.
pub(crate) fn persona_writer_tools(config: &Arc<Config>) -> Vec<Box<dyn Tool>> {
    vec![Box::new(WorkspaceUpdatePersonaTool::new(Arc::clone(
        config,
    )))]
}
