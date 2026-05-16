use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{Map, Value};

use crate::core::all::{ControllerFuture, RegisteredController};
use crate::core::ControllerSchema;
use crate::openhuman::config::rpc as config_rpc;
use crate::rpc::RpcOutcome;

#[derive(Debug, Deserialize)]
struct InferenceSummarizeParams {
    text: String,
    max_tokens: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct InferencePromptParams {
    prompt: String,
    max_tokens: Option<u32>,
    no_think: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct InferenceVisionPromptParams {
    prompt: String,
    image_refs: Vec<String>,
    max_tokens: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct InferenceEmbedParams {
    inputs: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct InferenceChatMessageParam {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct InferenceChatParams {
    messages: Vec<InferenceChatMessageParam>,
    max_tokens: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct InferenceShouldReactParams {
    message: String,
    channel_type: String,
}

#[derive(Debug, Deserialize)]
struct InferenceAnalyzeSentimentParams {
    message: String,
}

#[derive(Debug, Deserialize)]
struct InferenceShouldSendGifParams {
    message: String,
    channel_type: String,
}

#[derive(Debug, Deserialize)]
struct InferenceTenorSearchParams {
    query: String,
    limit: Option<u32>,
}

pub fn all_controller_schemas() -> Vec<ControllerSchema> {
    vec![
        schemas("status"),
        schemas("summarize"),
        schemas("prompt"),
        schemas("vision_prompt"),
        schemas("embed"),
        schemas("chat"),
        schemas("should_react"),
        schemas("analyze_sentiment"),
        schemas("should_send_gif"),
        schemas("tenor_search"),
    ]
}

pub fn all_registered_controllers() -> Vec<RegisteredController> {
    vec![
        RegisteredController {
            schema: schemas("status"),
            handler: handle_inference_status,
        },
        RegisteredController {
            schema: schemas("summarize"),
            handler: handle_inference_summarize,
        },
        RegisteredController {
            schema: schemas("prompt"),
            handler: handle_inference_prompt,
        },
        RegisteredController {
            schema: schemas("vision_prompt"),
            handler: handle_inference_vision_prompt,
        },
        RegisteredController {
            schema: schemas("embed"),
            handler: handle_inference_embed,
        },
        RegisteredController {
            schema: schemas("chat"),
            handler: handle_inference_chat,
        },
        RegisteredController {
            schema: schemas("should_react"),
            handler: handle_inference_should_react,
        },
        RegisteredController {
            schema: schemas("analyze_sentiment"),
            handler: handle_inference_analyze_sentiment,
        },
        RegisteredController {
            schema: schemas("should_send_gif"),
            handler: handle_inference_should_send_gif,
        },
        RegisteredController {
            schema: schemas("tenor_search"),
            handler: handle_inference_tenor_search,
        },
    ]
}

pub fn schemas(function: &str) -> ControllerSchema {
    let (source, target_function) = match function {
        "status" => (
            crate::openhuman::local_ai::local_ai_controller_schema("local_ai_status"),
            "status",
        ),
        "summarize" => (
            crate::openhuman::local_ai::local_ai_controller_schema("local_ai_summarize"),
            "summarize",
        ),
        "prompt" => (
            crate::openhuman::local_ai::local_ai_controller_schema("local_ai_prompt"),
            "prompt",
        ),
        "vision_prompt" => (
            crate::openhuman::local_ai::local_ai_controller_schema("local_ai_vision_prompt"),
            "vision_prompt",
        ),
        "embed" => (
            crate::openhuman::local_ai::local_ai_controller_schema("local_ai_embed"),
            "embed",
        ),
        "chat" => (
            crate::openhuman::local_ai::local_ai_controller_schema("local_ai_chat"),
            "chat",
        ),
        "should_react" => (
            crate::openhuman::local_ai::local_ai_controller_schema("local_ai_should_react"),
            "should_react",
        ),
        "analyze_sentiment" => (
            crate::openhuman::local_ai::local_ai_controller_schema("local_ai_analyze_sentiment"),
            "analyze_sentiment",
        ),
        "should_send_gif" => (
            crate::openhuman::local_ai::local_ai_controller_schema("local_ai_should_send_gif"),
            "should_send_gif",
        ),
        "tenor_search" => (
            crate::openhuman::local_ai::local_ai_controller_schema("local_ai_tenor_search"),
            "tenor_search",
        ),
        other => panic!("unknown inference schema: {other}"),
    };

    ControllerSchema {
        namespace: "inference",
        function: target_function,
        description: source.description,
        inputs: source.inputs,
        outputs: source.outputs,
    }
}

fn handle_inference_status(_params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout().await?;
        to_json(crate::openhuman::inference::rpc::inference_status(&config).await?)
    })
}

fn handle_inference_summarize(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let p = deserialize_params::<InferenceSummarizeParams>(params)?;
        let config = config_rpc::load_config_with_timeout().await?;
        to_json(
            crate::openhuman::inference::rpc::inference_summarize(&config, &p.text, p.max_tokens)
                .await?,
        )
    })
}

fn handle_inference_prompt(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let p = deserialize_params::<InferencePromptParams>(params)?;
        let config = config_rpc::load_config_with_timeout().await?;
        to_json(
            crate::openhuman::inference::rpc::inference_prompt(
                &config,
                &p.prompt,
                p.max_tokens,
                p.no_think,
            )
            .await?,
        )
    })
}

fn handle_inference_vision_prompt(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let p = deserialize_params::<InferenceVisionPromptParams>(params)?;
        let config = config_rpc::load_config_with_timeout().await?;
        to_json(
            crate::openhuman::inference::rpc::inference_vision_prompt(
                &config,
                &p.prompt,
                &p.image_refs,
                p.max_tokens,
            )
            .await?,
        )
    })
}

fn handle_inference_embed(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let p = deserialize_params::<InferenceEmbedParams>(params)?;
        let config = config_rpc::load_config_with_timeout().await?;
        to_json(crate::openhuman::inference::rpc::inference_embed(&config, &p.inputs).await?)
    })
}

fn handle_inference_chat(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let p = deserialize_params::<InferenceChatParams>(params)?;
        let config = config_rpc::load_config_with_timeout().await?;
        let messages = p
            .messages
            .into_iter()
            .map(
                |message| crate::openhuman::local_ai::ops::LocalAiChatMessage {
                    role: message.role,
                    content: message.content,
                },
            )
            .collect();
        to_json(
            crate::openhuman::inference::rpc::inference_chat(&config, messages, p.max_tokens)
                .await?,
        )
    })
}

fn handle_inference_should_react(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let p = deserialize_params::<InferenceShouldReactParams>(params)?;
        let config = config_rpc::load_config_with_timeout().await?;
        to_json(
            crate::openhuman::inference::rpc::inference_should_react(
                &config,
                &p.message,
                &p.channel_type,
            )
            .await?,
        )
    })
}

fn handle_inference_analyze_sentiment(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let p = deserialize_params::<InferenceAnalyzeSentimentParams>(params)?;
        let config = config_rpc::load_config_with_timeout().await?;
        to_json(
            crate::openhuman::inference::rpc::inference_analyze_sentiment(&config, &p.message)
                .await?,
        )
    })
}

fn handle_inference_should_send_gif(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let p = deserialize_params::<InferenceShouldSendGifParams>(params)?;
        let config = config_rpc::load_config_with_timeout().await?;
        to_json(
            crate::openhuman::inference::rpc::inference_should_send_gif(
                &config,
                &p.message,
                &p.channel_type,
            )
            .await?,
        )
    })
}

fn handle_inference_tenor_search(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let p = deserialize_params::<InferenceTenorSearchParams>(params)?;
        let config = config_rpc::load_config_with_timeout().await?;
        to_json(
            crate::openhuman::inference::rpc::inference_tenor_search(&config, &p.query, p.limit)
                .await?,
        )
    })
}

fn deserialize_params<T: DeserializeOwned>(params: Map<String, Value>) -> Result<T, String> {
    serde_json::from_value(Value::Object(params)).map_err(|e| format!("invalid params: {e}"))
}

fn to_json<T: serde::Serialize>(outcome: RpcOutcome<T>) -> Result<Value, String> {
    outcome.into_cli_compatible_json()
}

#[cfg(test)]
#[path = "schemas_tests.rs"]
mod tests;
