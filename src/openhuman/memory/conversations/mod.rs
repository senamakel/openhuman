//! Host-side wiring for workspace-backed conversation thread/message storage.
//!
//! Conversations are stored as JSONL files under the workspace (thread metadata
//! append-only in `threads.jsonl`; each thread's messages in a dedicated JSONL
//! file). The store / inverted-index / tokenizer / types engine is
//! `tinycortex::memory::conversations`, and consumers name that directly —
//! roughly a dozen files across `threads`, `channels` and `agent` already do.
//!
//! Host-retained:
//! - `bus` — the `core::bus` persistence subscriber that bridges typed channel
//!   events onto the store (the store abstracts the bus behind its own
//!   `ConversationEventBus` trait; the host wires the real one).
//! - [`blocking`] — `spawn_blocking` wrappers around the store's synchronous
//!   entry points. Request paths must use these, never the sync API (#5156).
//!
//! # What changed for #5560
//!
//! This module used to open with `pub use tinymemory_core::conversations::*;`,
//! and that glob carried exactly one item: `blocking`. The engine crate's own
//! docs already labelled it *host-retained* — nothing inside `tinymemory` ever
//! called it — so it moved here rather than being re-routed, and this file no
//! longer names the engine crate at all. See [`blocking`]'s module docs for the
//! one-line difference the move made.

pub mod blocking;

mod bus;

pub use bus::register_conversation_persistence_subscriber;
