//! Thin local-model RPC boundary backed by TinyAgents.
//!
//! Local runtimes execute out of process. OpenHuman only resolves their HTTP
//! endpoint and adapts the legacy local-AI call shape to TinyAgents' provider-
//! neutral `ChatModel` interface.

use crate::openhuman::config::Config;
use crate::openhuman::inference::local::lm_studio::lm_studio_base_url;
use crate::openhuman::inference::local::ollama::ollama_base_url_from_config;
use crate::openhuman::inference::local::provider::{provider_from_config, LocalAiProvider};
use tinyagents::harness::message::Message;
use tinyagents::harness::model::{ChatModel, ModelRequest};
use tinyagents::harness::providers::openai::OpenAiModel;

pub(super) struct ModelRpcOutcome {
    pub reply: String,
    pub prompt_tokens: Option<u64>,
    pub completion_tokens: Option<u64>,
}

fn local_model(config: &Config, model_id: &str) -> Result<OpenAiModel, String> {
    match provider_from_config(config) {
        LocalAiProvider::LmStudio => OpenAiModel::lm_studio(
            lm_studio_base_url(config),
            config.local_ai.api_key.as_deref().unwrap_or_default(),
            model_id,
        ),
        LocalAiProvider::Ollama => {
            OpenAiModel::ollama_at(ollama_base_url_from_config(config), model_id)
        }
    }
    .map_err(|error| format!("invalid local model RPC configuration: {error}"))
}

pub(super) async fn invoke(
    config: &Config,
    messages: Vec<Message>,
    max_tokens: Option<u32>,
    temperature: f32,
    allow_empty: bool,
) -> Result<ModelRpcOutcome, String> {
    let model_id = crate::openhuman::inference::model_ids::effective_chat_model_id(config);
    let model = local_model(config, &model_id)?;

    let mut request = ModelRequest::new(messages)
        .with_model(model_id)
        .with_temperature(temperature as f64);
    if let Some(max_tokens) = max_tokens {
        request = request.with_max_tokens(max_tokens);
    }

    let response = model
        .invoke(&(), request)
        .await
        .map_err(|error| format!("local model RPC failed: {error}"))?;
    let reply = response.text();
    let reply = reply.trim().to_owned();
    if reply.is_empty() && !allow_empty {
        return Err("local model RPC returned empty content".to_owned());
    }

    let prompt_tokens = response
        .usage
        .as_ref()
        .and_then(|usage| (usage.input_tokens > 0).then_some(usage.input_tokens));
    let completion_tokens = response
        .usage
        .as_ref()
        .and_then(|usage| (usage.output_tokens > 0).then_some(usage.output_tokens));

    Ok(ModelRpcOutcome {
        reply,
        prompt_tokens,
        completion_tokens,
    })
}
