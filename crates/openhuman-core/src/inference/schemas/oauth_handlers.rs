//! Handlers for the OpenAI OAuth flow: start, complete, Codex CLI import,
//! status, and disconnect.

use serde::Deserialize;
use serde_json::{Map, Value};

use super::{deserialize_params, to_json};
use crate::config::rpc as config_rpc;
use crate::core::all::ControllerFuture;

#[derive(Debug, Deserialize)]
pub(super) struct InferenceOpenAiOAuthCompleteParams {
    #[serde(alias = "callbackUrl")]
    callback_url: String,
}

pub(super) fn handle_inference_openai_oauth_start(_params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout().await?;
        to_json(crate::inference::rpc::inference_openai_oauth_start(&config).await?)
    })
}

pub(super) fn handle_inference_openai_oauth_complete(
    params: Map<String, Value>,
) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout().await?;
        let payload = deserialize_params::<InferenceOpenAiOAuthCompleteParams>(params)?;
        to_json(
            crate::inference::rpc::inference_openai_oauth_complete(
                &config,
                payload.callback_url.trim(),
            )
            .await?,
        )
    })
}

pub(super) fn handle_inference_openai_oauth_import_codex_cli(
    _params: Map<String, Value>,
) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout().await?;
        to_json(crate::inference::rpc::inference_openai_oauth_import_codex_cli(&config).await?)
    })
}

pub(super) fn handle_inference_openai_oauth_status(
    _params: Map<String, Value>,
) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout().await?;
        to_json(crate::inference::rpc::inference_openai_oauth_status(&config).await?)
    })
}

pub(super) fn handle_inference_openai_oauth_disconnect(
    _params: Map<String, Value>,
) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout().await?;
        to_json(crate::inference::rpc::inference_openai_oauth_disconnect(&config).await?)
    })
}
