//! `tools-fs-write` compiled out: no file-creating or file-modifying tools.

use std::path::Path;
use std::sync::Arc;

use tinytools::Tool;

use crate::config::{Config, HttpRequestConfig};
use crate::security::SecurityPolicy;

pub(crate) fn file_write_tools(
    _security: &Arc<SecurityPolicy>,
    _approval_workspace_root: Option<&Path>,
) -> Vec<Box<dyn Tool>> {
    tracing::debug!("[tools::capabilities] tools-fs-write compiled out; file_write not registered");
    Vec::new()
}

pub(crate) fn edit_tools(_security: &Arc<SecurityPolicy>) -> Vec<Box<dyn Tool>> {
    tracing::debug!(
        "[tools::capabilities] tools-fs-write compiled out; edit/apply_patch/csv_export not registered"
    );
    Vec::new()
}

pub(crate) fn curl_tools(
    _security: &Arc<SecurityPolicy>,
    _http_config: &HttpRequestConfig,
    _action_dir: &Path,
    _root_config: &Config,
) -> Vec<Box<dyn Tool>> {
    tracing::debug!("[tools::capabilities] tools-fs-write compiled out; curl not registered");
    Vec::new()
}
