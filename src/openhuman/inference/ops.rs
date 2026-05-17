//! JSON-RPC controller surface for inference operations.

use crate::openhuman::config::Config;
use crate::openhuman::local_ai;
use crate::openhuman::local_ai::ops::{LocalAiChatMessage, ReactionDecision};
use crate::openhuman::local_ai::sentiment::SentimentResult;
use crate::openhuman::local_ai::{LocalAiEmbeddingResult, LocalAiStatus};
use crate::rpc::RpcOutcome;
use tracing::{debug, error};

const LOG_PREFIX: &str = "[inference::ops]";

pub async fn inference_status(config: &Config) -> Result<RpcOutcome<LocalAiStatus>, String> {
    debug!("{LOG_PREFIX} status:start");
    let result = local_ai::rpc::local_ai_status(config).await;
    match &result {
        Ok(outcome) => debug!(state = %outcome.value.state, "{LOG_PREFIX} status:ok"),
        Err(err) => error!(error = %err, "{LOG_PREFIX} status:error"),
    }
    result
}

pub async fn inference_summarize(
    config: &Config,
    text: &str,
    max_tokens: Option<u32>,
) -> Result<RpcOutcome<String>, String> {
    debug!(
        text_len = text.len(),
        ?max_tokens,
        "{LOG_PREFIX} summarize:start"
    );
    let result = local_ai::rpc::local_ai_summarize(config, text, max_tokens).await;
    match &result {
        Ok(outcome) => debug!(
            output_len = outcome.value.len(),
            "{LOG_PREFIX} summarize:ok"
        ),
        Err(err) => error!(error = %err, "{LOG_PREFIX} summarize:error"),
    }
    result
}

pub async fn inference_prompt(
    config: &Config,
    prompt: &str,
    max_tokens: Option<u32>,
    no_think: Option<bool>,
) -> Result<RpcOutcome<String>, String> {
    debug!(
        prompt_len = prompt.len(),
        ?max_tokens,
        ?no_think,
        "{LOG_PREFIX} prompt:start"
    );
    let result = local_ai::rpc::local_ai_prompt(config, prompt, max_tokens, no_think).await;
    match &result {
        Ok(outcome) => debug!(output_len = outcome.value.len(), "{LOG_PREFIX} prompt:ok"),
        Err(err) => error!(error = %err, "{LOG_PREFIX} prompt:error"),
    }
    result
}

pub async fn inference_vision_prompt(
    config: &Config,
    prompt: &str,
    image_refs: &[String],
    max_tokens: Option<u32>,
) -> Result<RpcOutcome<String>, String> {
    debug!(
        prompt_len = prompt.len(),
        image_count = image_refs.len(),
        ?max_tokens,
        "{LOG_PREFIX} vision_prompt:start"
    );
    let result =
        local_ai::rpc::local_ai_vision_prompt(config, prompt, image_refs, max_tokens).await;
    match &result {
        Ok(outcome) => debug!(
            output_len = outcome.value.len(),
            "{LOG_PREFIX} vision_prompt:ok"
        ),
        Err(err) => error!(error = %err, "{LOG_PREFIX} vision_prompt:error"),
    }
    result
}

pub async fn inference_embed(
    config: &Config,
    inputs: &[String],
) -> Result<RpcOutcome<LocalAiEmbeddingResult>, String> {
    debug!(input_count = inputs.len(), "{LOG_PREFIX} embed:start");
    let result = local_ai::rpc::local_ai_embed(config, inputs).await;
    match &result {
        Ok(outcome) => debug!(
            vector_count = outcome.value.vectors.len(),
            dimensions = outcome.value.dimensions,
            "{LOG_PREFIX} embed:ok"
        ),
        Err(err) => error!(error = %err, "{LOG_PREFIX} embed:error"),
    }
    result
}

pub async fn inference_chat(
    config: &Config,
    messages: Vec<LocalAiChatMessage>,
    max_tokens: Option<u32>,
) -> Result<RpcOutcome<String>, String> {
    debug!(
        message_count = messages.len(),
        ?max_tokens,
        "{LOG_PREFIX} chat:start"
    );
    let result = local_ai::rpc::local_ai_chat(config, messages, max_tokens).await;
    match &result {
        Ok(outcome) => debug!(output_len = outcome.value.len(), "{LOG_PREFIX} chat:ok"),
        Err(err) => error!(error = %err, "{LOG_PREFIX} chat:error"),
    }
    result
}

pub async fn inference_should_react(
    config: &Config,
    message: &str,
    channel_type: &str,
) -> Result<RpcOutcome<ReactionDecision>, String> {
    debug!(
        message_len = message.len(),
        channel_type, "{LOG_PREFIX} should_react:start"
    );
    let result = local_ai::rpc::local_ai_should_react(config, message, channel_type).await;
    match &result {
        Ok(outcome) => debug!(
            should_react = outcome.value.should_react,
            "{LOG_PREFIX} should_react:ok"
        ),
        Err(err) => error!(error = %err, "{LOG_PREFIX} should_react:error"),
    }
    result
}

pub async fn inference_analyze_sentiment(
    config: &Config,
    message: &str,
) -> Result<RpcOutcome<SentimentResult>, String> {
    debug!(
        message_len = message.len(),
        "{LOG_PREFIX} analyze_sentiment:start"
    );
    let result = local_ai::sentiment::local_ai_analyze_sentiment(config, message).await;
    match &result {
        Ok(outcome) => {
            debug!(valence = %outcome.value.valence, "{LOG_PREFIX} analyze_sentiment:ok")
        }
        Err(err) => error!(error = %err, "{LOG_PREFIX} analyze_sentiment:error"),
    }
    result
}

#[cfg(test)]
#[path = "ops_tests.rs"]
mod tests;
