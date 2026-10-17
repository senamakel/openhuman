//! Host-side browser policy and TinyComputer adapters.

pub(crate) mod chrome_hints;
mod client;
mod policy;
mod task;
mod task_report;

pub use client::{BrowserCallError, BrowserClient, MODULE_ID};
pub use task::{cancel, report, resume, start, start_request, wait, BrowserTask};
