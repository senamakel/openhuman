//! Terminal chat UI — the `openhuman tui` (alias `chat`) CLI subcommand.
//!
//! A [ratatui]-based terminal front-end onto the **same `web_chat` surface**
//! the desktop app drives (`openhuman.channel_web_chat` /
//! `openhuman.channel_web_cancel` +
//! [`web_chat::subscribe_web_channel_events`](crate::openhuman::web_chat::subscribe_web_channel_events)).
//! It boots the core in-process — no HTTP, no sockets — via
//! `CoreBuilder::new(HostKind::Cli).domains(DomainSet::full()).services(ServiceSet::none())`
//! and streams a live transcript in the terminal.
//!
//! ## Compile-time gate (`tui` feature)
//!
//! `pub mod tui;` is ALWAYS compiled — it is a facade (mirrors
//! [`mcp_server`](crate::openhuman::mcp_server)). The terminal driver, the
//! renderer, the reducer, and the event loop are gated behind the default-ON
//! `tui` Cargo feature; when it is off, [`stub`] mirrors the one surface an
//! always-compiled caller reaches — [`run_from_cli`] — with a build-fact
//! disabled-error body.
//!
//! The `"tui" | "chat"` arm in `src/core/cli.rs` is deliberately left
//! **un-`#[cfg]`'d**: in a slim build it resolves to [`stub::run_from_cli`],
//! which bails with a message naming the compile-time gate as the cause (so the
//! error reads like a build fact, not `unknown namespace: tui`). This is the
//! same reasoning documented on [`mcp_server::stub::run_stdio_from_cli`].

#[cfg(feature = "tui")]
mod app;
#[cfg(feature = "tui")]
mod render;
#[cfg(feature = "tui")]
mod runner;
#[cfg(feature = "tui")]
mod state;
#[cfg(feature = "tui")]
mod terminal;

#[cfg(feature = "tui")]
pub use runner::run_from_cli;

// State reducer is behaviour-only but has no terminal deps, so its tests run in
// the default (feature-on) build. Exported for the sibling submodules + tests.
#[cfg(feature = "tui")]
pub use state::{Entry, EntryKind, TranscriptState};

// ---------------------------------------------------------------------------
// Disabled facade — compiled only when the `tui` feature is OFF.
// ---------------------------------------------------------------------------

#[cfg(not(feature = "tui"))]
mod stub;
#[cfg(not(feature = "tui"))]
pub use stub::*;
