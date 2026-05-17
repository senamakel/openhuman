use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{Map, Value};

use crate::core::all::{ControllerFuture, RegisteredController};
use crate::core::{ControllerSchema, FieldSchema, TypeSchema};
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
    ]
}

pub fn schemas(function: &str) -> ControllerSchema {
    match function {
        "status" => ControllerSchema {
            namespace: "inference",
            function: "status",
            description: "Read inference service status.",
            inputs: vec![],
            outputs: vec![json_output("status", "Inference status payload.")],
        },
        "summarize" => ControllerSchema {
            namespace: "inference",
            function: "summarize",
            description: "Summarize text with the configured inference provider.",
            inputs: vec![
                required_string("text", "Input text."),
                optional_u64("max_tokens", "Optional max output tokens."),
            ],
            outputs: vec![json_output("summary", "Summary text.")],
        },
        "prompt" => ControllerSchema {
            namespace: "inference",
            function: "prompt",
            description: "Run a direct inference prompt.",
            inputs: vec![
                required_string("prompt", "Prompt text."),
                optional_u64("max_tokens", "Optional max output tokens."),
                optional_bool("no_think", "Disable thinking mode."),
            ],
            outputs: vec![json_output("output", "Prompt output text.")],
        },
        "vision_prompt" => ControllerSchema {
            namespace: "inference",
            function: "vision_prompt",
            description: "Run a multimodal inference prompt with image refs.",
            inputs: vec![
                required_string("prompt", "Prompt text."),
                FieldSchema {
                    name: "image_refs",
                    ty: TypeSchema::Array(Box::new(TypeSchema::String)),
                    comment: "Image references to include.",
                    required: true,
                },
                optional_u64("max_tokens", "Optional max output tokens."),
            ],
            outputs: vec![json_output("output", "Prompt output text.")],
        },
        "embed" => ControllerSchema {
            namespace: "inference",
            function: "embed",
            description: "Generate embeddings for text inputs.",
            inputs: vec![FieldSchema {
                name: "inputs",
                ty: TypeSchema::Array(Box::new(TypeSchema::String)),
                comment: "Texts to embed.",
                required: true,
            }],
            outputs: vec![json_output("embedding", "Embedding result payload.")],
        },
        "chat" => ControllerSchema {
            namespace: "inference",
            function: "chat",
            description: "Multi-turn chat completion via the configured inference provider.",
            inputs: vec![
                FieldSchema {
                    name: "messages",
                    ty: TypeSchema::Array(Box::new(TypeSchema::Json)),
                    comment: "Chat message history [{role, content}]. Last entry is the user turn.",
                    required: true,
                },
                optional_u64("max_tokens", "Optional max output tokens."),
            ],
            outputs: vec![json_output("reply", "Assistant reply text.")],
        },
        "should_react" => ControllerSchema {
            namespace: "inference",
            function: "should_react",
            description: "Ask the inference provider whether the assistant should add an emoji reaction to a user message, based on channel type.",
            inputs: vec![
                required_string("message", "User message content to evaluate."),
                required_string("channel_type", "Channel type: web, telegram, discord, slack, etc."),
            ],
            outputs: vec![json_output("decision", "Reaction decision: {should_react, emoji}.")],
        },
        "analyze_sentiment" => ControllerSchema {
            namespace: "inference",
            function: "analyze_sentiment",
            description: "Classify the emotion and valence of a user message with the inference provider.",
            inputs: vec![required_string("message", "User message content to classify.")],
            outputs: vec![json_output("sentiment", "Sentiment analysis payload.")],
        },
        other => panic!("unknown inference schema: {other}"),
    }
}

fn required_string(name: &'static str, comment: &'static str) -> FieldSchema {
    FieldSchema {
        name,
        ty: TypeSchema::String,
        comment,
        required: true,
    }
}

fn optional_bool(name: &'static str, comment: &'static str) -> FieldSchema {
    FieldSchema {
        name,
        ty: TypeSchema::Option(Box::new(TypeSchema::Bool)),
        comment,
        required: false,
    }
}

fn optional_u64(name: &'static str, comment: &'static str) -> FieldSchema {
    FieldSchema {
        name,
        ty: TypeSchema::Option(Box::new(TypeSchema::U64)),
        comment,
        required: false,
    }
}

fn json_output(name: &'static str, comment: &'static str) -> FieldSchema {
    FieldSchema {
        name,
        ty: TypeSchema::Json,
        comment,
        required: true,
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

fn deserialize_params<T: DeserializeOwned>(params: Map<String, Value>) -> Result<T, String> {
    serde_json::from_value(Value::Object(params)).map_err(|e| format!("invalid params: {e}"))
}

fn to_json<T: serde::Serialize>(outcome: RpcOutcome<T>) -> Result<Value, String> {
    outcome.into_cli_compatible_json()
}

#[cfg(test)]
#[path = "schemas_tests.rs"]
mod tests;
