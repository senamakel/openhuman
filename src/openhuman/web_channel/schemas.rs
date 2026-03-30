use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{Map, Value};

use crate::core::all::{ControllerFuture, RegisteredController};
use crate::core::{ControllerSchema, FieldSchema, TypeSchema};

#[derive(Debug, Deserialize)]
struct WebChatParams {
    client_id: String,
    thread_id: String,
    message: String,
    model_override: Option<String>,
    temperature: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct WebCancelParams {
    client_id: String,
    thread_id: String,
}

pub fn all_controller_schemas() -> Vec<ControllerSchema> {
    vec![schemas("chat"), schemas("cancel")]
}

pub fn all_registered_controllers() -> Vec<RegisteredController> {
    vec![
        RegisteredController {
            schema: schemas("chat"),
            handler: handle_chat,
        },
        RegisteredController {
            schema: schemas("cancel"),
            handler: handle_cancel,
        },
    ]
}

pub fn schemas(function: &str) -> ControllerSchema {
    match function {
        "chat" => ControllerSchema {
            namespace: "channel",
            function: "web_chat",
            description: "Send a web channel message through the agent loop.",
            inputs: vec![
                required_string("client_id", "Client stream identifier."),
                required_string("thread_id", "Thread identifier."),
                required_string("message", "User message."),
                optional_string("model_override", "Optional model override."),
                optional_f64("temperature", "Optional temperature override."),
            ],
            outputs: vec![json_output("ack", "Acceptance payload.")],
        },
        "cancel" => ControllerSchema {
            namespace: "channel",
            function: "web_cancel",
            description: "Cancel in-flight web channel request for a thread.",
            inputs: vec![
                required_string("client_id", "Client stream identifier."),
                required_string("thread_id", "Thread identifier."),
            ],
            outputs: vec![json_output("ack", "Cancellation payload.")],
        },
        _ => ControllerSchema {
            namespace: "channel",
            function: "unknown",
            description: "Unknown web channel controller function.",
            inputs: vec![],
            outputs: vec![FieldSchema {
                name: "error",
                ty: TypeSchema::String,
                comment: "Lookup error details.",
                required: true,
            }],
        },
    }
}

fn handle_chat(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let p = deserialize_params::<WebChatParams>(params)?;
        to_json(
            crate::openhuman::web_channel::ops::channel_web_chat(
                &p.client_id,
                &p.thread_id,
                &p.message,
                p.model_override,
                p.temperature,
            )
            .await?,
        )
    })
}

fn handle_cancel(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let p = deserialize_params::<WebCancelParams>(params)?;
        to_json(
            crate::openhuman::web_channel::ops::channel_web_cancel(&p.client_id, &p.thread_id)
                .await?,
        )
    })
}

fn deserialize_params<T: DeserializeOwned>(params: Map<String, Value>) -> Result<T, String> {
    serde_json::from_value(Value::Object(params)).map_err(|e| format!("invalid params: {e}"))
}

fn required_string(name: &'static str, comment: &'static str) -> FieldSchema {
    FieldSchema {
        name,
        ty: TypeSchema::String,
        comment,
        required: true,
    }
}

fn optional_string(name: &'static str, comment: &'static str) -> FieldSchema {
    FieldSchema {
        name,
        ty: TypeSchema::Option(Box::new(TypeSchema::String)),
        comment,
        required: false,
    }
}

fn optional_f64(name: &'static str, comment: &'static str) -> FieldSchema {
    FieldSchema {
        name,
        ty: TypeSchema::Option(Box::new(TypeSchema::F64)),
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

fn to_json<T: serde::Serialize>(outcome: crate::rpc::RpcOutcome<T>) -> Result<Value, String> {
    outcome.into_cli_compatible_json()
}
