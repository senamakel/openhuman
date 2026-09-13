//! URL-based skill installation: fetch, validate, and write SKILL.md from a remote URL.

#[cfg(test)]
#[path = "ops_install_install_fetch_tests_tests.rs"]
mod install_fetch_tests;

mod fetch;
mod uninstall;
mod url_validation;

pub use fetch::{
    install_workflow_from_url, InstallWorkflowFromUrlOutcome, InstallWorkflowFromUrlParams,
    DEFAULT_INSTALL_TIMEOUT_SECS, MAX_INSTALL_TIMEOUT_SECS, MAX_WORKFLOW_MD_BYTES,
};
pub use uninstall::{uninstall_workflow, UninstallWorkflowOutcome, UninstallWorkflowParams};
pub use url_validation::{validate_install_url, validate_resolved_host, MAX_INSTALL_URL_LEN};

#[cfg(test)]
pub(crate) use fetch::{install_workflow_from_url_with_home, should_report_install_fetch_status};
#[cfg(test)]
pub(crate) use url_validation::{derive_install_slug, normalize_install_url};
