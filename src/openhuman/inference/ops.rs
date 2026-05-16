//! JSON-RPC controller surface for inference operations.

use crate::openhuman::config::Config;
use crate::openhuman::local_ai;
use crate::openhuman::local_ai::gif_decision::GifDecision;
use crate::openhuman::local_ai::ops::{LocalAiChatMessage, ReactionDecision};
use crate::openhuman::local_ai::sentiment::SentimentResult;
use crate::openhuman::local_ai::{LocalAiEmbeddingResult, LocalAiStatus, TenorSearchResult};
use crate::rpc::RpcOutcome;

pub async fn inference_status(config: &Config) -> Result<RpcOutcome<LocalAiStatus>, String> {
    local_ai::rpc::local_ai_status(config).await
}

pub async fn inference_summarize(
    config: &Config,
    text: &str,
    max_tokens: Option<u32>,
) -> Result<RpcOutcome<String>, String> {
    local_ai::rpc::local_ai_summarize(config, text, max_tokens).await
}

pub async fn inference_prompt(
    config: &Config,
    prompt: &str,
    max_tokens: Option<u32>,
    no_think: Option<bool>,
) -> Result<RpcOutcome<String>, String> {
    local_ai::rpc::local_ai_prompt(config, prompt, max_tokens, no_think).await
}

pub async fn inference_vision_prompt(
    config: &Config,
    prompt: &str,
    image_refs: &[String],
    max_tokens: Option<u32>,
) -> Result<RpcOutcome<String>, String> {
    local_ai::rpc::local_ai_vision_prompt(config, prompt, image_refs, max_tokens).await
}

pub async fn inference_embed(
    config: &Config,
    inputs: &[String],
) -> Result<RpcOutcome<LocalAiEmbeddingResult>, String> {
    local_ai::rpc::local_ai_embed(config, inputs).await
}

pub async fn inference_chat(
    config: &Config,
    messages: Vec<LocalAiChatMessage>,
    max_tokens: Option<u32>,
) -> Result<RpcOutcome<String>, String> {
    local_ai::rpc::local_ai_chat(config, messages, max_tokens).await
}

pub async fn inference_should_react(
    config: &Config,
    message: &str,
    channel_type: &str,
) -> Result<RpcOutcome<ReactionDecision>, String> {
    local_ai::rpc::local_ai_should_react(config, message, channel_type).await
}

pub async fn inference_analyze_sentiment(
    config: &Config,
    message: &str,
) -> Result<RpcOutcome<SentimentResult>, String> {
    local_ai::sentiment::local_ai_analyze_sentiment(config, message).await
}

pub async fn inference_should_send_gif(
    config: &Config,
    message: &str,
    channel_type: &str,
) -> Result<RpcOutcome<GifDecision>, String> {
    local_ai::gif_decision::local_ai_should_send_gif(config, message, channel_type).await
}

pub async fn inference_tenor_search(
    config: &Config,
    query: &str,
    limit: Option<u32>,
) -> Result<RpcOutcome<TenorSearchResult>, String> {
    local_ai::gif_decision::tenor_search(config, query, limit).await
}

#[cfg(test)]
#[path = "ops_tests.rs"]
mod tests;
