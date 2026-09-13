//! Podcast generation + email delivery (`audio_toolkit` RPC namespace).
//!
//! See [README.md](README.md) for responsibilities, the RPC surface, and the
//! agent tools this module exposes.

mod ops;
mod schemas;
pub mod tools;
mod types;

pub use ops::{
    email_podcast, generate_and_email_podcast, generate_podcast, resolve_email_capture_dir,
};
pub use schemas::{all_audio_toolkit_controller_schemas, all_audio_toolkit_registered_controllers};
pub use types::{
    AudioEmailDeliveryResult, AudioFormat, AudioGenerateRequest, AudioGeneratedArtifact,
    AudioToolkitGenerateAndEmailResult, EmailPodcastRequest,
};
