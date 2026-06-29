//! Checkpointing — durable pause/resume for graph runs (issue #4249).
//!
//! A [`Checkpointer`] persists run records + state snapshots so a long-running
//! or human-in-the-loop graph can pause and later resume with full context.
//! [`InMemoryCheckpointer`] backs tests; [`SqliteCheckpointer`] is the durable
//! production backend (DB under `{workspace}/.openhuman/agent_graph/`).

mod checkpointer;
mod store;
mod types;

pub use checkpointer::{Checkpointer, InMemoryCheckpointer};
pub use store::SqliteCheckpointer;
pub use types::{Checkpoint, GraphRunRecord};
