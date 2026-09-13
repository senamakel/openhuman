//! Cross-channel integration suite, declared as
//! `#[cfg(all(feature = "channels", test))] mod tests` in `channels/mod.rs`
//! (see AGENTS.md — unit tests otherwise stay beside their modules as
//! `*_tests.rs`; this tree is the deliberate exception for tests that
//! exercise more than one channel submodule end-to-end).
//!
//! `common.rs` holds the shared fixtures the files below build on: fake
//! models per scenario (`DummyModel`, `SlowModel`, `ToolCallingModel`,
//! `IterativeToolModel`, `HistoryCaptureModel`, `ModelCaptureModel`), fake
//! `Channel` implementations that record sends or always fail
//! (`RecordingChannel`, `TelegramRecordingChannel`, `AlwaysFailChannel`), a
//! `NoopMemory`, a `MockPriceTool`, `make_workspace` for identity-file
//! fixtures, and a re-export of `agent::bus::use_real_agent_handler`.
//!
//! `context.rs` sits in this directory but is not declared below, so it does
//! not compile; the live coverage for `channels/context.rs` is
//! `channels/context_tests.rs`.
//!
//! * `discord_integration.rs` — end-to-end dispatch through the Discord
//!   channel with every cross-module boundary (agent runtime, memory,
//!   provider) substituted, proving the domain stays encapsulated.
//! * `health.rs` — `commands::classify_health_result` and
//!   `spawn_supervised_listener` marking a component errored and restarting
//!   when a listener keeps failing.
//! * `identity.rs` — `build_system_prompt` inlining workspace identity
//!   markdown into the Project Context section.
//! * `memory.rs` — conversation-history and memory-context wiring through
//!   `process_channel_message`.
//! * `personality.rs` — acceptance coverage for #6027/#6028 (channel turns
//!   carry the active personality; identity edits reach the next turn
//!   without a restart).
//! * `prompt.rs` — system-prompt section assembly and bootstrap truncation.
//! * `runtime_dispatch.rs` — the dispatch loop through
//!   `runtime::test_support::run_dispatch_harness`: inbound-envelope
//!   publication, parallel processing, typing-task cancellation, the
//!   `agent.run_turn` bus route, and multimodal path-marker hardening.
//! * `runtime_tool_calls.rs` — native tool-call execution, the `/models`
//!   command short-circuit, route overrides, and `max_tool_iterations`
//!   through `process_channel_message`.
//! * `telegram_integration.rs` — Telegram reactions, reply/thread roundtrip,
//!   and typing-indicator lifecycle against a recording channel.

mod common;
mod discord_integration;
mod health;
mod identity;
mod memory;
mod personality;
mod prompt;
mod runtime_dispatch;
mod runtime_tool_calls;
mod telegram_integration;
