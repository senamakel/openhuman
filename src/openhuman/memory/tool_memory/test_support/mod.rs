//! The engine crate's tool-memory test fixtures, re-exported for this tree.
//!
//! One line, and it lives in its own directory on purpose. `MockMemory` is a
//! `tinymemory-core` fixture compiled behind `cfg(any(test, feature =
//! "test-support"))`, reached from four inline `#[cfg(test)]` modules —
//! `memory::tool_memory::capture`, `agent::experience::{capture, store}` and
//! `agent::tinyagents::host::experience_store`. `cfg(test)` code links the
//! `tinymemory-core` **dev-dependency** (declared with `features =
//! ["test-support"]` at `Cargo.toml`'s `[dev-dependencies]`), which survives
//! #5560's shed — so this names the engine crate without putting a byte of it
//! in the shipped binary.
//!
//! # Why it is not in the parent `mod.rs`
//!
//! It was, as a `#[cfg(test)] pub use`, and the parent's own docs already
//! explained that it is dev-only. `memory::direct_engine_refs_tests` could not
//! see that explanation: its scanner reads line by line and skips only comments
//! and whole files, deliberately, because brace-tracking inline `#[cfg(test)]`
//! blocks is the complexity that lint's docs say it does not want. So a
//! genuinely test-only reference kept a production file on the direct-reference
//! allowlist, which made "deliberate" and "not migrated yet" look identical in
//! the one list that exists to tell them apart.
//!
//! `test_support/` is the escape hatch both scanners already honour by path
//! (`is_test_path` in `direct_engine_refs_tests` and in `bypass_allowlist_tests`
//! each match on a `test_support` path *component*, which is why this is a
//! directory and not a `test_support.rs`). Moving the line here classifies it
//! rather than hiding it: the reference is unchanged, still `#[cfg(test)]`, and
//! still the dev-dependency's.
//!
//! The public path callers use is unchanged —
//! `memory::tool_memory::test_helpers::MockMemory` — because the parent
//! re-exports this module's contents under that name.

pub use tinymemory_core::tool_memory::test_helpers;
