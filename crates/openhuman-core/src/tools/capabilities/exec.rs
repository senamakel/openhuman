//! `tools-exec`: the tools that run programs other than the shell.

use std::path::Path;
use std::sync::Arc;

use tinytools::Tool;
use tinytools_std::detect_tools::DetectToolsTool;
use tinytools_std::filesystem::{GitOperationsTool, RunLinterTool, RunTestsTool};

use crate::agent::host_runtime::RuntimeAdapter;
use crate::config::Config;
use crate::runtime::python::PythonBootstrap;
use crate::security::SecurityPolicy;
use crate::tools::{InstallToolTool, PythonExecTool};

/// `detect_tools` and `install_tool`.
pub(crate) fn toolchain_tools(security: &Arc<SecurityPolicy>) -> Vec<Box<dyn Tool>> {
    vec![
        Box::new(DetectToolsTool::new()),
        Box::new(InstallToolTool::new(Arc::clone(security))),
    ]
}

/// `git_operations`, rooted at the agent's action dir.
pub(crate) fn git_tools(security: &Arc<SecurityPolicy>, action_dir: &Path) -> Vec<Box<dyn Tool>> {
    vec![Box::new(GitOperationsTool::new(
        Arc::clone(security),
        action_dir.to_path_buf(),
    ))]
}

/// `run_linter` and `run_tests`: the review loop over the action dir.
pub(crate) fn review_tools(action_dir: &Path) -> Vec<Box<dyn Tool>> {
    vec![
        Box::new(RunLinterTool::new(action_dir.to_path_buf())),
        Box::new(RunTestsTool::new(action_dir.to_path_buf())),
    ]
}

/// `python_exec`, only while the managed Python runtime is enabled. Shares the
/// session's `PythonBootstrap` with the shell; inline code routes through the
/// shared runtime pool (#5106).
pub(crate) fn python_tools(
    security: &Arc<SecurityPolicy>,
    runtime: &Arc<dyn RuntimeAdapter>,
    python_bootstrap: Option<&Arc<PythonBootstrap>>,
    root_config: &Config,
) -> Vec<Box<dyn Tool>> {
    let Some(bootstrap) = python_bootstrap else {
        return Vec::new();
    };
    tracing::debug!("[tools::ops] registered python_exec");
    vec![Box::new(PythonExecTool::new(
        Arc::clone(security),
        Arc::clone(runtime),
        Arc::clone(bootstrap),
        root_config.runtime_pool.clone(),
        root_config.workspace_dir.clone(),
    ))]
}
