//! JSON-RPC / CLI controller surface for the bundled local AI stack.
//!
//! This module provides high-level functions for interacting with local AI
//! services such as agent chat, model downloads, summarization, and
//! transcription. These functions are typically invoked via RPC or CLI.

#[cfg(test)]
#[path = "ops_tests.rs"]
mod tests;

mod agent_chat;
mod chat;
mod reactions;
mod runtime_ops;
mod turn_guards;

pub use agent_chat::{agent_chat, agent_chat_simple};
pub use chat::{local_ai_chat, LocalAiChatMessage};
pub use reactions::{local_ai_should_react, ReactionDecision};
pub use runtime_ops::{
    local_ai_assets_status, local_ai_download_asset, local_ai_downloads_progress, local_ai_embed,
    local_ai_prompt, local_ai_status, local_ai_summarize, local_ai_transcribe,
    local_ai_transcribe_bytes, local_ai_tts, local_ai_vision_prompt,
};

#[cfg(test)]
use crate::config::Config;
#[cfg(test)]
use reactions::{extract_first_emoji, is_emoji_start};
#[cfg(test)]
use turn_guards::{
    effective_agent_chat_origin, grant_turn_cwd, normalize_model_override, resolve_turn_cwd,
};
