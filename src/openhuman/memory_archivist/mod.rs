//! Conversation archivist — captures chat turns to disk as Obsidian-compatible
//! markdown files. Placeholder for the FTS5 episodic surface that lives in
//! `memory_store::unified::fts5` today; the long-term plan is to retire
//! `unified/` and let this module own all episodic capture, backed by the
//! raw md content store + grep / vector retrieval.
//!
//! ## Layout on disk
//!
//! Every turn lands at:
//! ```text
//! <content_root>/episodic/<session_id>/<turn_seq>.md
//! ```
//! with YAML front-matter (session, role, timestamp_ms, cost_microdollars,
//! lesson, tool_calls) and the turn `content` as the body. Bodies are
//! immutable after the first write — same contract as `content::atomic`.
//!
//! ## Read path
//!
//! - [`session_entries`] walks `<content_root>/episodic/<session>/` in seq
//!   order and parses each file. Returns the canonical [`ArchivedTurn`]
//!   shape — a thin serde mirror of `EpisodicEntry`.
//! - Cross-session search is intentionally not implemented here yet — the
//!   right answer is to grep the md files, which slots in once the grep
//!   tooling lands.

pub mod store;
pub mod types;

pub use store::{record_turn, session_entries};
pub use types::ArchivedTurn;
