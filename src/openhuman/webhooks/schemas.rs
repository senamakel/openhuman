use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{Map, Value};

use crate::core::all::{ControllerFuture, RegisteredController};
use crate::core::{ControllerSchema, FieldSchema, TypeSchema};
use crate::rpc::RpcOutcome;

#[derive(Debug, Deserialize)]
struct WebhookListLogsParams {
    limit: Option<usize>,
}

pub fn all_controller_schemas() -> Vec<ControllerSchema> {
    vec![
        schemas("list_registrations"),
        schemas("list_logs"),
        schemas("clear_logs"),
    ]
}

pub fn all_registered_controllers() -> Vec<RegisteredController> {
    vec![
        RegisteredController {
            schema: schemas("list_registrations"),
            handler: handle_list_registrations,
        },
        RegisteredController {
            schema: schemas("list_logs"),
            handler: handle_list_logs,
        },
        RegisteredController {
            schema: schemas("clear_logs"),
            handler: handle_clear_logs,
        },
    ]
}

pub fn schemas(function: &str) -> ControllerSchema {
    match function {
        "list_registrations" => ControllerSchema {
            namespace: "webhooks",
            function: "list_registrations",
            description:
                "List all webhook tunnel registrations currently owned by the app runtime.",
            inputs: vec![],
            outputs: vec![json_output("result", "Webhook registration list.")],
        },
        "list_logs" => ControllerSchema {
            namespace: "webhooks",
            function: "list_logs",
            description: "List captured webhook request and response debug logs.",
            inputs: vec![FieldSchema {
                name: "limit",
                ty: TypeSchema::Option(Box::new(TypeSchema::U64)),
                comment: "Maximum number of log entries to return.",
                required: false,
            }],
            outputs: vec![json_output("result", "Webhook debug log list.")],
        },
        "clear_logs" => ControllerSchema {
            namespace: "webhooks",
            function: "clear_logs",
            description: "Clear captured webhook debug logs.",
            inputs: vec![],
            outputs: vec![json_output("result", "Webhook log clear result.")],
        },
        _ => ControllerSchema {
            namespace: "webhooks",
            function: "unknown",
            description: "Unknown webhooks controller function.",
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

fn handle_list_registrations(_params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async { to_json(crate::openhuman::webhooks::ops::list_registrations().await?) })
}

fn handle_list_logs(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let payload = deserialize_params::<WebhookListLogsParams>(params)?;
        to_json(crate::openhuman::webhooks::ops::list_logs(payload.limit).await?)
    })
}

fn handle_clear_logs(_params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async { to_json(crate::openhuman::webhooks::ops::clear_logs().await?) })
}

fn deserialize_params<T: DeserializeOwned>(params: Map<String, Value>) -> Result<T, String> {
    serde_json::from_value(Value::Object(params)).map_err(|e| format!("invalid params: {e}"))
}

fn to_json<T: serde::Serialize>(outcome: RpcOutcome<T>) -> Result<Value, String> {
    outcome.into_cli_compatible_json()
}

fn json_output(name: &'static str, comment: &'static str) -> FieldSchema {
    FieldSchema {
        name,
        ty: TypeSchema::Json,
        comment,
        required: true,
    }
}
