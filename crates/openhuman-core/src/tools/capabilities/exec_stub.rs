//! `tools-exec` compiled out: no program-execution tools besides the shell.

use std::path::Path;
use std::sync::Arc;

use tinytools::Tool;

use crate::agent::host_runtime::RuntimeAdapter;
use crate::config::Config;
use crate::runtime::python::PythonBootstrap;
use crate::security::SecurityPolicy;

pub(crate) fn toolchain_tools(_security: &Arc<SecurityPolicy>) -> Vec<Box<dyn Tool>> {
    tracing::debug!(
        "[tools::capabilities] tools-exec compiled out; detect_tools/install_tool not registered"
    );
    Vec::new()
}

pub(crate) fn git_tools(_security: &Arc<SecurityPolicy>, _action_dir: &Path) -> Vec<Box<dyn Tool>> {
    tracing::debug!("[tools::capabilities] tools-exec compiled out; git_operations not registered");
    Vec::new()
}

pub(crate) fn review_tools(_action_dir: &Path) -> Vec<Box<dyn Tool>> {
    tracing::debug!(
        "[tools::capabilities] tools-exec compiled out; run_linter/run_tests not registered"
    );
    Vec::new()
}

pub(crate) fn python_tools(
    _security: &Arc<SecurityPolicy>,
    _runtime: &Arc<dyn RuntimeAdapter>,
    _python_bootstrap: Option<&Arc<PythonBootstrap>>,
    _root_config: &Config,
) -> Vec<Box<dyn Tool>> {
    tracing::debug!("[tools::capabilities] tools-exec compiled out; python_exec not registered");
    Vec::new()
}
