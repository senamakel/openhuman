//! `tools-fs-write`: the tools that create or modify files.

use std::path::Path;
use std::sync::Arc;

use tinytools::Tool;
use tinytools_std::filesystem::{ApplyPatchTool, CsvExportTool, EditFileTool, FileWriteTool};
use tinytools_std::network::CurlTool;

use crate::config::{Config, HttpRequestConfig};
use crate::security::SecurityPolicy;

/// `file_write`, scoped to the approval workspace root when the session has one.
pub(crate) fn file_write_tools(
    security: &Arc<SecurityPolicy>,
    approval_workspace_root: Option<&Path>,
) -> Vec<Box<dyn Tool>> {
    let tool: Box<dyn Tool> = match approval_workspace_root {
        Some(root) => Box::new(FileWriteTool::with_approval_workspace_root(
            Arc::clone(security),
            root.to_path_buf(),
        )),
        None => Box::new(FileWriteTool::new(Arc::clone(security))),
    };
    vec![tool]
}

/// `edit`, `apply_patch` and `csv_export`.
pub(crate) fn edit_tools(security: &Arc<SecurityPolicy>) -> Vec<Box<dyn Tool>> {
    vec![
        Box::new(EditFileTool::new(Arc::clone(security))),
        Box::new(ApplyPatchTool::new(Arc::clone(security))),
        Box::new(CsvExportTool::new(Arc::clone(security))),
    ]
}

/// `curl`: shares `http_request.allowed_domains` and streams downloads under
/// `<action_dir>/<curl.dest_subdir>` with a hard byte ceiling.
pub(crate) fn curl_tools(
    security: &Arc<SecurityPolicy>,
    http_config: &HttpRequestConfig,
    action_dir: &Path,
    root_config: &Config,
) -> Vec<Box<dyn Tool>> {
    vec![Box::new(CurlTool::new(
        Arc::clone(security),
        http_config.allowed_domains.clone(),
        action_dir.to_path_buf(),
        root_config.curl.dest_subdir.clone(),
        root_config.curl.max_download_bytes,
        root_config.curl.timeout_secs,
    ))]
}
