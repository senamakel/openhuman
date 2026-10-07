//! Registration facades for the capability features.
//!
//! Five Cargo features remove agent-tool families that act on the host from a
//! build: `tools-shell`, `tools-fs-write`, `tools-exec`, `tools-system` (here)
//! and `composio` (`integrations::composio::tools`). Each family module has a
//! real body and a `*_stub.rs` twin selected by its feature, so
//! [`all_tools_with_runtime`](super::ops::all_tools_with_runtime) calls the
//! same functions in every build and needs no `#[cfg]` of its own. A stub
//! returns an empty list: the tools are absent, not present-and-refusing.
//!
//! Each family is split into the slices the registry interleaves with other
//! tools, so the assembled order — and with it the tool list a provider sees —
//! is the same as before the gates existed.
//!
//! Signatures MUST match between a module and its stub. The trimmed build
//! (`cargo check -p openhuman --no-default-features --features skills,modules`)
//! is what catches drift.

#[cfg(feature = "tools-shell")]
pub(crate) mod shell;
#[cfg(not(feature = "tools-shell"))]
#[path = "shell_stub.rs"]
pub(crate) mod shell;

#[cfg(feature = "tools-fs-write")]
pub(crate) mod fs_write;
#[cfg(not(feature = "tools-fs-write"))]
#[path = "fs_write_stub.rs"]
pub(crate) mod fs_write;

#[cfg(feature = "tools-exec")]
pub(crate) mod exec;
#[cfg(not(feature = "tools-exec"))]
#[path = "exec_stub.rs"]
pub(crate) mod exec;

#[cfg(feature = "tools-system")]
pub(crate) mod system;
#[cfg(not(feature = "tools-system"))]
#[path = "system_stub.rs"]
pub(crate) mod system;
