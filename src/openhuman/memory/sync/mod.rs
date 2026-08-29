//! Host layer over the memory sync domain.
//!
//! What lives here is the JSON-RPC surface — handlers and controller schemas
//! name OpenHuman's `RpcOutcome` and `ControllerSchema`, which no engine crate
//! can see. The two submodules below are that surface.
//!
//! # Why there is no `pub use tinymemory_core::sync::*;` here any more (#5560)
//!
//! There was one, described as keeping "every historical `memory::sync::…` path
//! resolving". It resolved nothing. The glob's only reachable contribution was
//! four engine modules — `audit`, `mcp`, `pipelines`, `workspace` — because the
//! other two names it carried (`composio`, `sync_status`) are declared below
//! and an explicit item shadows a glob import. A repo-wide search for those
//! four under this path found **no consumer**: not in `src/`, not in `tests/`,
//! not in the desktop shell's separate Cargo world. Every real caller reaches
//! the engine's sync pipelines by naming `tinycortex::memory::sync::…` (see
//! `integrations::composio::ops::memory_cleanup`) or goes through
//! `memory::sync::composio`, which is its own module.
//!
//! So this line was a compile-time edge to `tinymemory-core` bought for nobody,
//! and dropping it is not a carve-out or a substitution — nothing resolved
//! through it, so nothing changes shape. The sibling shims that *are* load
//! bearing (`composio`, `composio::providers`, `composio::providers::slack`)
//! keep theirs and say what pins them.
//!
//! Note that `sync_status` made the opposite move earlier and for a different
//! reason: its glob carried two real names, and they were repointed at
//! `tinycortex`, which defines them. See that module's own docs.

pub mod composio;
pub mod sync_status;
