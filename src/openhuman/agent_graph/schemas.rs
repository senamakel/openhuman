//! Controller schemas + handlers for the `agent_graph` domain.
//!
//! RPC method names: `openhuman.agent_graph_<function>`. Handlers delegate to
//! [`ops`](super::ops); business logic lives there.

use serde_json::{Map, Value};

use crate::core::all::{ControllerFuture, RegisteredController};
use crate::core::{ControllerSchema, FieldSchema, TypeSchema};
use crate::openhuman::config::rpc as config_rpc;
use crate::rpc::RpcOutcome;

/// Schemas for the controller registry (metadata only).
pub fn all_controller_schemas() -> Vec<ControllerSchema> {
    vec![
        schemas("definition_list"),
        schemas("agent_list"),
        schemas("agent_graph"),
        schemas("run"),
        schemas("run_list"),
        schemas("run_get"),
        schemas("checkpoint_list"),
        schemas("resume"),
    ]
}

/// Schema + handler pairs for registration.
pub fn all_registered_controllers() -> Vec<RegisteredController> {
    vec![
        RegisteredController {
            schema: schemas("definition_list"),
            handler: handle_definition_list,
        },
        RegisteredController {
            schema: schemas("agent_list"),
            handler: handle_agent_list,
        },
        RegisteredController {
            schema: schemas("agent_graph"),
            handler: handle_agent_graph,
        },
        RegisteredController {
            schema: schemas("run"),
            handler: handle_run,
        },
        RegisteredController {
            schema: schemas("run_list"),
            handler: handle_run_list,
        },
        RegisteredController {
            schema: schemas("run_get"),
            handler: handle_run_get,
        },
        RegisteredController {
            schema: schemas("checkpoint_list"),
            handler: handle_checkpoint_list,
        },
        RegisteredController {
            schema: schemas("resume"),
            handler: handle_resume,
        },
    ]
}

fn run_id_input() -> FieldSchema {
    FieldSchema {
        name: "run_id",
        ty: TypeSchema::String,
        comment: "The graph run id.",
        required: true,
    }
}

/// Central schema definitions.
pub fn schemas(function: &str) -> ControllerSchema {
    match function {
        "definition_list" => ControllerSchema {
            namespace: "agent_graph",
            function: "definition_list",
            description: "List the registered agent-graph definitions (name, nodes, HITL).",
            inputs: vec![],
            outputs: vec![],
        },
        "agent_list" => ControllerSchema {
            namespace: "agent_graph",
            function: "agent_list",
            description: "List every built-in agent's LangGraph-compatible execution chain.",
            inputs: vec![],
            outputs: vec![],
        },
        "agent_graph" => ControllerSchema {
            namespace: "agent_graph",
            function: "agent_graph",
            description: "Fetch one built-in agent's execution-chain blueprint by id.",
            inputs: vec![FieldSchema {
                name: "agent_id",
                ty: TypeSchema::String,
                comment: "Built-in agent id (e.g. 'orchestrator', 'researcher').",
                required: true,
            }],
            outputs: vec![],
        },
        "run" => ControllerSchema {
            namespace: "agent_graph",
            function: "run",
            description: "Start a run of a named graph definition.",
            inputs: vec![
                FieldSchema {
                    name: "name",
                    ty: TypeSchema::String,
                    comment: "Graph definition name (see agent_graph_definition_list).",
                    required: true,
                },
                FieldSchema {
                    name: "input",
                    ty: TypeSchema::Option(Box::new(TypeSchema::Json)),
                    comment: "Seed variables object (e.g. { task, auto_approve }).",
                    required: false,
                },
            ],
            outputs: vec![],
        },
        "run_list" => ControllerSchema {
            namespace: "agent_graph",
            function: "run_list",
            description: "List graph runs, newest-first, paged.",
            inputs: vec![
                FieldSchema {
                    name: "limit",
                    ty: TypeSchema::Option(Box::new(TypeSchema::U64)),
                    comment: "Max runs (1-500, default 50).",
                    required: false,
                },
                FieldSchema {
                    name: "offset",
                    ty: TypeSchema::Option(Box::new(TypeSchema::U64)),
                    comment: "Pagination offset (default 0).",
                    required: false,
                },
            ],
            outputs: vec![],
        },
        "run_get" => ControllerSchema {
            namespace: "agent_graph",
            function: "run_get",
            description: "Fetch one graph run with its node transitions and checkpoints.",
            inputs: vec![run_id_input()],
            outputs: vec![],
        },
        "checkpoint_list" => ControllerSchema {
            namespace: "agent_graph",
            function: "checkpoint_list",
            description: "List the checkpoints for a graph run.",
            inputs: vec![run_id_input()],
            outputs: vec![],
        },
        "resume" => ControllerSchema {
            namespace: "agent_graph",
            function: "resume",
            description: "Resume a paused human-in-the-loop run with the human's input.",
            inputs: vec![
                run_id_input(),
                FieldSchema {
                    name: "input",
                    ty: TypeSchema::String,
                    comment: "The human's answer (e.g. 'approve' / 'reject' / free text).",
                    required: true,
                },
            ],
            outputs: vec![],
        },
        other => panic!("unknown agent_graph controller function '{other}'"),
    }
}

fn to_json<T: serde::Serialize>(outcome: RpcOutcome<T>) -> Result<Value, String> {
    outcome.into_cli_compatible_json()
}

fn read_required_str(params: &Map<String, Value>, key: &str) -> Result<String, String> {
    params
        .get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| format!("missing required string param '{key}'"))
}

fn read_optional_usize(params: &Map<String, Value>, key: &str) -> Option<usize> {
    params.get(key).and_then(|v| v.as_u64()).map(|n| n as usize)
}

fn handle_definition_list(_params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move { to_json(super::ops::definition_list()?) })
}

fn handle_agent_list(_params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move { to_json(super::ops::agent_list()?) })
}

fn handle_agent_graph(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let agent_id = read_required_str(&params, "agent_id")?;
        to_json(super::ops::agent_graph(agent_id.trim())?)
    })
}

fn handle_run(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout().await?;
        let name = read_required_str(&params, "name")?;
        let input = params.get("input").cloned().unwrap_or(Value::Null);
        to_json(super::ops::run(&config, name.trim(), input).await?)
    })
}

fn handle_run_list(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout().await?;
        let limit = read_optional_usize(&params, "limit");
        let offset = read_optional_usize(&params, "offset");
        to_json(super::ops::run_list(&config, limit, offset).await?)
    })
}

fn handle_run_get(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout().await?;
        let run_id = read_required_str(&params, "run_id")?;
        to_json(super::ops::run_get(&config, run_id.trim()).await?)
    })
}

fn handle_checkpoint_list(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout().await?;
        let run_id = read_required_str(&params, "run_id")?;
        to_json(super::ops::checkpoint_list(&config, run_id.trim()).await?)
    })
}

fn handle_resume(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout().await?;
        let run_id = read_required_str(&params, "run_id")?;
        let input = read_required_str(&params, "input")?;
        to_json(super::ops::resume(&config, run_id.trim(), &input).await?)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_schemas_have_agent_graph_namespace() {
        for schema in all_controller_schemas() {
            assert_eq!(schema.namespace, "agent_graph");
        }
    }

    #[test]
    fn registered_controllers_match_schema_count() {
        assert_eq!(
            all_controller_schemas().len(),
            all_registered_controllers().len()
        );
    }

    #[test]
    fn rpc_method_names_are_namespaced() {
        let s = schemas("run_list");
        assert_eq!(
            crate::core::all::rpc_method_name(&s),
            "openhuman.agent_graph_run_list"
        );
    }
}
