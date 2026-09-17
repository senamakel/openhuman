//! Session persistence: transcript loading, checkpointing, and background tasks.
//!
//! Each submodule contributes an `impl Agent` block:
//!
//! - [`transcript_load`] — resume-time transcript lookup and read.
//! - [`transcript_persist`] — post-turn transcript write and store mirrors.
//! - [`wrapup`] — out-of-band wrap-up summary and required-output repair.
//! - [`background_tasks`] — fire-and-forget memory extraction and ingestion.

mod background_tasks;
mod transcript_load;
mod transcript_persist;
mod wrapup;
