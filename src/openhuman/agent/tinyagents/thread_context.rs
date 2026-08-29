//! Ambient chat-thread id for the current async scope.
//!
//! The definition **moved to `tinymemory_core::thread_context`** during the
//! memory extraction. The memory store is its heaviest consumer — recall has to
//! exclude the thread it is being called from — and the task-local cannot be
//! two different task-locals in two crates or the scope would not be visible
//! across the seam.
//!
//! It could not go in `tinymemory-api`, which must not depend on an async
//! runtime; `tokio::task_local!` needs one. Every existing
//! `agent::tinyagents::thread_context::…` path keeps resolving and keeps
//! naming the same task-local.
//!
//! # Bringing the definition home is a Phase F move, not a free one (#5560)
//!
//! This looks like a shim that could be replaced by a host-side
//! `tokio::task_local!` in one commit. It cannot, while the in-process engine
//! is still linked, and the failure would be silent.
//!
//! The engine reads this task-local in exactly one production place:
//! `tinymemory_core::store::recall_policy::current_self_echo_exclusion`, which
//! is how an in-process recall stops returning the very thread it was called
//! from. Two `task_local!` invocations are two distinct keys, so moving the
//! definition here would leave the engine's key permanently unset — and unset
//! means *exclude nothing*, a permission-shaped check that fails open. Recall
//! would start echoing the caller's own thread back at it, with no error
//! anywhere.
//!
//! That read is already dead in module mode, for the reason `AGENTS.md` states:
//! a task-local set in this process is invisible inside a separately compiled
//! `cdylib`, so callers must pass `RecallOpts::exclude_session_id` explicitly
//! and the host's own call sites do. So the definition comes home **with** the
//! engine's removal, in the same commit — not before it.

pub use tinymemory_core::thread_context::{current_thread_id, with_thread_id};
