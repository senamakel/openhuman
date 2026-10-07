//! `tools-shell`: the `shell` tool.

use std::sync::Arc;

use tinytools::Tool;

use crate::agent::host_runtime::RuntimeAdapter;
use crate::runtime::javascript::NodeBootstrap;
use crate::runtime::python::PythonBootstrap;
use crate::security::{AuditLogger, SecurityPolicy};
use crate::tools::ShellTool;

/// The shell, wired to the session's managed language runtimes when present.
pub(crate) fn shell_tools(
    security: &Arc<SecurityPolicy>,
    runtime: &Arc<dyn RuntimeAdapter>,
    audit: &Arc<AuditLogger>,
    node_bootstrap: Option<&Arc<NodeBootstrap>>,
    python_bootstrap: Option<&Arc<PythonBootstrap>>,
) -> Vec<Box<dyn Tool>> {
    vec![Box::new(ShellTool::with_language_bootstraps(
        Arc::clone(security),
        Arc::clone(runtime),
        Arc::clone(audit),
        node_bootstrap.map(Arc::clone),
        python_bootstrap.map(Arc::clone),
    ))]
}
