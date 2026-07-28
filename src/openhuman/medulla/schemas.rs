//! Controller schemas for the `medulla` RPC namespace.
//!
//! Handlers delegate straight to [`super::ops`]; no business logic lives here.
//! Registered under [`DomainGroup::Medulla`](crate::core::all::DomainGroup) at
//! the single site in `src/core/all.rs`, so a host that switches the family off
//! sees these methods as unknown rather than as failing.

use serde_json::{Map, Value};

use crate::core::all::{ControllerFuture, RegisteredController};
use crate::core::{ControllerSchema, FieldSchema, TypeSchema};

use super::ops;

/// Every schema in the namespace, for `/schema` introspection.
pub fn all_medulla_controller_schemas() -> Vec<ControllerSchema> {
    vec![
        medulla_schemas("medulla_status"),
        medulla_schemas("medulla_list_sessions"),
        medulla_schemas("medulla_roster"),
    ]
}

/// Every controller in the namespace, for dispatch.
pub fn all_medulla_registered_controllers() -> Vec<RegisteredController> {
    vec![
        RegisteredController {
            schema: medulla_schemas("medulla_status"),
            handler: handle_status,
        },
        RegisteredController {
            schema: medulla_schemas("medulla_list_sessions"),
            handler: handle_list_sessions,
        },
        RegisteredController {
            schema: medulla_schemas("medulla_roster"),
            handler: handle_roster,
        },
    ]
}

/// Schema for one function in the namespace.
pub fn medulla_schemas(function: &str) -> ControllerSchema {
    match function {
        "medulla_status" => ControllerSchema {
            namespace: "medulla",
            function: "status",
            description: "Whether the Medulla integration is configured and signed in. Never performs a network call.",
            inputs: vec![],
            outputs: vec![FieldSchema {
                name: "status",
                ty: TypeSchema::Json,
                comment: "Readiness: configured flag, resolved base URL, session-token presence, and a stable reason when unconfigured.",
                required: true,
            }],
        },
        "medulla_list_sessions" => ControllerSchema {
            namespace: "medulla",
            function: "list_sessions",
            description: "List the operator's durable Medulla sessions.",
            inputs: vec![],
            outputs: vec![FieldSchema {
                name: "sessions",
                ty: TypeSchema::Array(Box::new(TypeSchema::Json)),
                comment: "Session summaries ordered by the backend.",
                required: true,
            }],
        },
        "medulla_roster" => ControllerSchema {
            namespace: "medulla",
            function: "roster",
            description: "Read the roster of workers currently connected to the Medulla backend.",
            inputs: vec![],
            outputs: vec![FieldSchema {
                name: "workers",
                ty: TypeSchema::Array(Box::new(TypeSchema::Json)),
                comment: "Connected worker entries.",
                required: true,
            }],
        },
        other => panic!("unknown medulla controller function: {other}"),
    }
}

fn handle_status(_params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move { to_json(ops::status(&load_config().await?).await?) })
}

fn handle_list_sessions(_params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move { to_json(ops::list_sessions(&load_config().await?).await?) })
}

fn handle_roster(_params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move { to_json(ops::roster(&load_config().await?).await?) })
}

/// Load the ambient config for a handler.
async fn load_config() -> Result<crate::openhuman::config::Config, String> {
    crate::openhuman::config::ops::load_config_with_timeout().await
}

/// Serialize an outcome through the shared CLI-compatible envelope.
fn to_json<T: serde::Serialize>(outcome: crate::rpc::RpcOutcome<T>) -> Result<Value, String> {
    outcome.into_cli_compatible_json()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_declared_schema_has_a_registered_controller() {
        let schemas = all_medulla_controller_schemas();
        let controllers = all_medulla_registered_controllers();
        assert_eq!(
            schemas.len(),
            controllers.len(),
            "a declared schema without a handler is unreachable, and vice versa"
        );
        for (schema, controller) in schemas.iter().zip(controllers.iter()) {
            assert_eq!(schema.namespace, controller.schema.namespace);
            assert_eq!(schema.function, controller.schema.function);
        }
    }

    #[test]
    fn all_schemas_share_the_medulla_namespace() {
        for schema in all_medulla_controller_schemas() {
            assert_eq!(schema.namespace, "medulla");
            assert!(!schema.description.is_empty());
        }
    }

    #[test]
    fn rpc_method_names_follow_the_crate_convention() {
        let names: Vec<String> = all_medulla_registered_controllers()
            .iter()
            .map(|c| c.rpc_method_name())
            .collect();
        assert_eq!(
            names,
            vec![
                "openhuman.medulla_status",
                "openhuman.medulla_list_sessions",
                "openhuman.medulla_roster",
            ]
        );
    }
}
