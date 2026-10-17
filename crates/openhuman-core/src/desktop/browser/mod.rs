//! Host-side browser policy and TinyComputer adapters.

mod chrome_hints;
mod client;
mod policy;
mod task;
mod task_report;

pub(crate) use chrome_hints::{
    chrome_hint, chrome_not_found, chrome_path_unusable, CHROME_NOT_FOUND_HINT, CHROME_PATH_HINT,
};
pub use client::{BrowserCallError, BrowserClient, MODULE_ID};
pub(crate) use task::{cancel, report, resume, start, start_request, wait, BrowserTask};
