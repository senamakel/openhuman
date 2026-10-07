//! `tools-system` compiled out: no host-process or host-settings tools.

use std::sync::Arc;

use tinytools::Tool;

use crate::config::Config;
use crate::security::SecurityPolicy;

pub(crate) fn proxy_tools(
    _config: &Arc<Config>,
    _security: &Arc<SecurityPolicy>,
) -> Vec<Box<dyn Tool>> {
    tracing::debug!("[tools::capabilities] tools-system compiled out; proxy_config not registered");
    Vec::new()
}

pub(crate) fn update_apply_tools(_security: &Arc<SecurityPolicy>) -> Vec<Box<dyn Tool>> {
    tracing::debug!("[tools::capabilities] tools-system compiled out; update_apply not registered");
    Vec::new()
}

pub(crate) fn service_lifecycle_tools(_config: &Arc<Config>) -> Vec<Box<dyn Tool>> {
    tracing::debug!(
        "[tools::capabilities] tools-system compiled out; service lifecycle tools not registered"
    );
    Vec::new()
}

pub(crate) fn persona_writer_tools(_config: &Arc<Config>) -> Vec<Box<dyn Tool>> {
    tracing::debug!(
        "[tools::capabilities] tools-system compiled out; workspace_update_persona not registered"
    );
    Vec::new()
}
