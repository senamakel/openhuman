//! Controller schemas for the `medulla_local` RPC namespace.
//!
//! Registered in `src/core/all.rs` under `DomainGroup::Agent` behind the
//! `medulla-local` feature. Two methods this draft: `status` and `instruct`.

use serde_json::{Map, Value};
use tracing::warn;

use crate::core::all::{ControllerFuture, RegisteredController};
use crate::core::{ControllerSchema, FieldSchema, TypeSchema};
use crate::openhuman::medulla_local::ops::{instruct_handler, status_handler, InstructParams};

pub fn all_controller_schemas() -> Vec<ControllerSchema> {
    vec![
        schemas("medulla_local_status"),
        schemas("medulla_local_instruct"),
    ]
}

pub fn all_registered_controllers() -> Vec<RegisteredController> {
    vec![
        RegisteredController {
            schema: schemas("medulla_local_status"),
            handler: handle_status,
        },
        RegisteredController {
            schema: schemas("medulla_local_instruct"),
            handler: handle_instruct,
        },
    ]
}

pub fn schemas(function: &str) -> ControllerSchema {
    match function {
        "medulla_local_status" => ControllerSchema {
            namespace: "medulla_local",
            function: "status",
            description: "Status of the supervised local medulla-serve child: whether it is connected, its serve version, session id, and negotiated port set.",
            inputs: vec![],
            outputs: vec![FieldSchema {
                name: "status",
                ty: TypeSchema::Json,
                comment: "MedullaLocalStatus: {enabled, running, serve_version?, session_id?, ports, message?}.",
                required: true,
            }],
        },
        "medulla_local_instruct" => ControllerSchema {
            namespace: "medulla_local",
            function: "instruct",
            description: "Enqueue one instruction against the local medulla-serve harness. Returns the synchronous receipt; the cycle runs async and is observed via the event stream.",
            inputs: vec![
                FieldSchema {
                    name: "message",
                    ty: TypeSchema::String,
                    comment: "The instruction text for the harness cycle.",
                    required: true,
                },
                FieldSchema {
                    name: "meta",
                    ty: TypeSchema::Option(Box::new(TypeSchema::Json)),
                    comment: "Optional instruction metadata (e.g. {origin: 'wake'}).",
                    required: false,
                },
            ],
            outputs: vec![
                FieldSchema {
                    name: "instructionId",
                    ty: TypeSchema::String,
                    comment: "Id of the enqueued instruction.",
                    required: true,
                },
                FieldSchema {
                    name: "cycleId",
                    ty: TypeSchema::String,
                    comment: "Id of the cycle the instruction will run in.",
                    required: true,
                },
            ],
        },
        _ => ControllerSchema {
            namespace: "medulla_local",
            function: "unknown",
            description: "Unknown medulla_local controller.",
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

fn handle_status(_params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move { status_handler().await })
}

fn handle_instruct(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let params: InstructParams =
            serde_json::from_value(Value::Object(params)).map_err(|error| {
                warn!("[medulla_local] medulla_local.instruct rejected malformed params: {error}");
                error.to_string()
            })?;
        instruct_handler(params).await
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_lists_status_and_instruct() {
        let schemas = all_controller_schemas();
        assert_eq!(schemas.len(), 2);
        let names: Vec<&str> = schemas.iter().map(|schema| schema.function).collect();
        assert!(names.contains(&"status"));
        assert!(names.contains(&"instruct"));
    }

    #[test]
    fn instruct_schema_requires_message() {
        let schema = schemas("medulla_local_instruct");
        assert_eq!(schema.namespace, "medulla_local");
        assert!(schema
            .inputs
            .iter()
            .any(|field| field.name == "message" && field.required));
    }
}
