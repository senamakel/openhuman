//! Stateful agent session — the single execution tier.
//!
//! This module owns the [`OpenHumanSessionHost`] struct, which drives per-turn
//! interaction with the provider, tool registry, memory system, and
//! hook pipeline. It is the runtime the `channels`, `local_ai`, and
//! `cron` layers invoke when they need a conversation to make
//! progress.
//!
//! # Product shell over TinyAgents
//!
//! Every production turn is a `tinyagents_runtime::Session` transition. The
//! OpenHuman driver supplies model/tool execution; codec and hooks supply the
//! product transcript dialect, prompt policy, and post-durability effects.
//! The runtime, rather than this host, owns generic history, raw transcript
//! reconciliation, prefix stability, tool snapshots, resume, and persistence.
//!
//! # File layout
//!
//! | File          | Role                                                             |
//! |---------------|------------------------------------------------------------------|
//! | [`types`]     | `OpenHumanSessionHost` and `SessionHostBuilder` struct definitions (no logic).        |
//! | [`builder`]   | `SessionHostBuilder` fluent API + `OpenHumanSessionHost::from_config` factory.        |
//! | [`runtime_session`] | Runtime session composition and public `turn()`.          |
//! | [`driver`]    | OpenHuman model/harness `SessionDriver`.                         |
//! | [`hooks`]     | Product preparation/commit/terminal hooks.                       |
//! | [`runtime`]   | Public accessors and `run_single` / `run_interactive`.           |
//!
//! External callers should import [`OpenHumanSessionHost`] and [`SessionHostBuilder`] from
//! `crate::agent`, which re-exports them from this module.
//! The child files are an implementation detail.

#[cfg(feature = "flows")]
pub(crate) use builder::provider_role_for_definition;
pub use builder::{HostOnlyToolPolicy, HostTools, HostTurnTools, TurnContext};

mod builder;
mod builder_config;
mod codec;
mod driver;
// Tool surface for live voice sessions (`voice::live`).
#[cfg(feature = "voice")]
mod live_tools;
#[cfg(test)]
#[path = "resume_bound_session_tests.rs"]
mod resume_bound_session_tests;
mod runtime;
mod runtime_session;
pub(crate) mod stop_summary;
mod turn;
// `pub(crate)` since issue #6014: the tool-call-cap instruction is now appended
// inside the loop by `tinyagents::middleware::FinalCallWrapUpMiddleware`, so the
// harness-assembly site has to name it. It stays the one definition — the whole
// point is that the in-loop conclusion and the out-of-band fallback ask for the
// same thing.
pub(crate) mod turn_checkpoint;
mod types;

pub use codec::OpenHumanTranscriptCodec;
pub type OpenHumanSessionHooks =
    tinyagents_runtime::FnSessionHooks<crate::agent::tinyagents::host::OpenHumanRunContext>;
pub use types::{OpenHumanSessionHost, SessionHostBuilder, TurnOverrides};

// Re-export the duplicate-tool-spec guard for sibling harness modules
// (`session::runtime`, `subagent_host`) so all provider call sites
// share one tested implementation.
pub(crate) use builder::dedup_visible_tool_specs;

#[cfg(test)]
mod orphaned_head_resume_tests;
#[cfg(test)]
mod response_shape_turn_tests;
#[cfg(test)]
mod runtime_adapter_session_state_tests;
#[cfg(test)]
mod runtime_adapter_tests;
#[cfg(test)]
mod runtime_adapter_usage_tests;
#[cfg(test)]
mod transcript_compat_tests;
#[cfg(test)]
#[path = "transcript_resume_stable_tests.rs"]
mod transcript_resume_stable_tests;
#[cfg(test)]
mod typed_transcript_compat_tests;
