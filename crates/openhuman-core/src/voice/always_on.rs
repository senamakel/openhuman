//! Phase 2 — always-on listening.
//!
//! Instead of a hotkey gating each recording, always-on mode keeps the mic
//! open continuously and uses **voice-activity detection** to carve the audio
//! stream into utterances: an utterance opens when energy rises above an onset
//! threshold and closes after a configurable run of silence (the "hangover").
//! Each completed utterance is transcribed and pushed onto the dictation bus,
//! so it reaches the agent and the notch exactly like a hotkey dictation.
//!
//! ## Where the work happens
//!
//! Everything that is not device I/O runs in the `tinyvoice` module — the
//! segmenter, the downmix, the resample, the per-frame energies, the WAV
//! framing, the wake-word gate and the intent classifier. This file owns the
//! `cpal` stream, the thread discipline around it, and the policy decisions
//! (what to transcribe, when to pause, what to tell the notch).
//!
//! The split follows the audio callback, not the cost of a call. A module call
//! is ~15 µs against a 20 ms frame, so the bus is not the constraint; the
//! constraint is that `cpal` delivers on a realtime thread where blocking is a
//! dropout. So the callback does the least it can — convert the sample format
//! and forward raw interleaved samples — and every transform happens in the
//! async processor below.
//!
//! Privacy: always-on is **opt-in** (`config.voice_server.always_on_enabled`,
//! default false) and pauses when the screen is locked.
//!
//! ## Module layout
//!
//! - [`capture`] — the `cpal` stream and its realtime callback.
//! - [`lock_watcher`] — the macOS screen-lock privacy hook.
//! - [`processor`] — the async pipeline: VAD session, segmentation, gating.
//! - [`transcribe`] — STT, wake-word gating, and local fast-path commands.

mod capture;
mod lock_watcher;
mod processor;
mod transcribe;

pub use processor::{start_if_enabled, stop};

use processor::notch_status;
#[cfg(target_os = "macos")]
use processor::PAUSED;

#[cfg(test)]
use crate::modules::voice as tinyvoice;
#[cfg(test)]
use processor::ENABLED;

#[cfg(test)]
#[path = "always_on_tests.rs"]
mod tests;

const LOG_PREFIX: &str = "[voice::always_on]";
