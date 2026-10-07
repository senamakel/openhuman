//! `tools-shell` compiled out: no `shell` tool.

use std::sync::Arc;

use tinytools::Tool;

use crate::agent::host_runtime::RuntimeAdapter;
use crate::runtime::javascript::NodeBootstrap;
use crate::runtime::python::PythonBootstrap;
use crate::security::{AuditLogger, SecurityPolicy};

pub(crate) fn shell_tools(
    _security: &Arc<SecurityPolicy>,
    _runtime: &Arc<dyn RuntimeAdapter>,
    _audit: &Arc<AuditLogger>,
    _node_bootstrap: Option<&Arc<NodeBootstrap>>,
    _python_bootstrap: Option<&Arc<PythonBootstrap>>,
) -> Vec<Box<dyn Tool>> {
    tracing::debug!("[tools::capabilities] tools-shell compiled out; shell not registered");
    Vec::new()
}
