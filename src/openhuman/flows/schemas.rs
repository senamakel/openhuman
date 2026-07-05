//! RPC/CLI controller surface for the `flows::` domain. Mirrors
//! `src/openhuman/cron/schemas.rs`'s shape exactly: `schemas(function)` builds
//! one `ControllerSchema`, `all_controller_schemas()`/
//! `all_registered_controllers()` aggregate them, and each `handle_*` loads
//! config, reads params, awaits the matching `ops::flows_*` fn, and converts
//! the `RpcOutcome` to CLI-compatible JSON.

use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

use crate::core::all::{ControllerFuture, RegisteredController};
use crate::core::{ControllerSchema, FieldSchema, TypeSchema};
use crate::openhuman::config::rpc as config_rpc;
use crate::openhuman::flows::ops;
use crate::rpc::RpcOutcome;

fn id_input(comment: &'static str) -> FieldSchema {
    FieldSchema {
        name: "id",
        ty: TypeSchema::String,
        comment,
        required: true,
    }
}

fn flow_output() -> FieldSchema {
    FieldSchema {
        name: "flow",
        ty: TypeSchema::Ref("Flow"),
        comment: "The flow definition.",
        required: true,
    }
}

fn require_approval_input() -> FieldSchema {
    FieldSchema {
        name: "require_approval",
        ty: TypeSchema::Option(Box::new(TypeSchema::Bool)),
        comment: "Force a human-approval gate on every outbound tool/HTTP action this flow \
                  takes, regardless of its saved-flow trust root. Defaults to `false`.",
        required: false,
    }
}

fn run_output_fields() -> Vec<FieldSchema> {
    vec![
        FieldSchema {
            name: "output",
            ty: TypeSchema::Json,
            comment: "The run's final state (per-node items, trigger payload).",
            required: true,
        },
        FieldSchema {
            name: "pending_approvals",
            ty: TypeSchema::Array(Box::new(TypeSchema::String)),
            comment: "Node ids paused awaiting human approval; empty once completed.",
            required: true,
        },
        FieldSchema {
            name: "thread_id",
            ty: TypeSchema::String,
            comment: "Durable checkpoint thread id for this run (needed to resume).",
            required: true,
        },
    ]
}

pub fn all_controller_schemas() -> Vec<ControllerSchema> {
    vec![
        schemas("create"),
        schemas("validate"),
        schemas("get"),
        schemas("list"),
        schemas("update"),
        schemas("delete"),
        schemas("set_enabled"),
        schemas("run"),
        schemas("resume"),
        schemas("cancel_run"),
        schemas("list_runs"),
        schemas("get_run"),
    ]
}

pub fn all_registered_controllers() -> Vec<RegisteredController> {
    vec![
        RegisteredController {
            schema: schemas("create"),
            handler: handle_create,
        },
        RegisteredController {
            schema: schemas("validate"),
            handler: handle_validate,
        },
        RegisteredController {
            schema: schemas("get"),
            handler: handle_get,
        },
        RegisteredController {
            schema: schemas("list"),
            handler: handle_list,
        },
        RegisteredController {
            schema: schemas("update"),
            handler: handle_update,
        },
        RegisteredController {
            schema: schemas("delete"),
            handler: handle_delete,
        },
        RegisteredController {
            schema: schemas("set_enabled"),
            handler: handle_set_enabled,
        },
        RegisteredController {
            schema: schemas("run"),
            handler: handle_run,
        },
        RegisteredController {
            schema: schemas("resume"),
            handler: handle_resume,
        },
        RegisteredController {
            schema: schemas("cancel_run"),
            handler: handle_cancel_run,
        },
        RegisteredController {
            schema: schemas("list_runs"),
            handler: handle_list_runs,
        },
        RegisteredController {
            schema: schemas("get_run"),
            handler: handle_get_run,
        },
    ]
}

pub fn schemas(function: &str) -> ControllerSchema {
    match function {
        "create" => ControllerSchema {
            namespace: "flows",
            function: "create",
            description: "Create a new saved automation workflow from a tinyflows graph.",
            inputs: vec![
                FieldSchema {
                    name: "name",
                    ty: TypeSchema::String,
                    comment: "Human-readable flow name.",
                    required: true,
                },
                FieldSchema {
                    name: "graph",
                    ty: TypeSchema::Json,
                    comment:
                        "A tinyflows WorkflowGraph (nodes + edges); validated and migrated on save.",
                    required: true,
                },
                require_approval_input(),
            ],
            outputs: vec![flow_output()],
        },
        "validate" => ControllerSchema {
            namespace: "flows",
            function: "validate",
            description: "Validate a tinyflows graph without saving it: reports structural \
                          validity plus non-fatal warnings (e.g. a trigger kind that does not \
                          fire automatically yet).",
            inputs: vec![FieldSchema {
                name: "graph",
                ty: TypeSchema::Json,
                comment: "A tinyflows WorkflowGraph (nodes + edges) to validate and migrate.",
                required: true,
            }],
            outputs: vec![
                FieldSchema {
                    name: "valid",
                    ty: TypeSchema::Bool,
                    comment: "True when the graph is structurally valid.",
                    required: true,
                },
                FieldSchema {
                    name: "errors",
                    ty: TypeSchema::Array(Box::new(TypeSchema::String)),
                    comment: "Structural validation errors; empty when `valid`.",
                    required: true,
                },
                FieldSchema {
                    name: "warnings",
                    ty: TypeSchema::Array(Box::new(TypeSchema::String)),
                    comment: "Non-fatal warnings (e.g. an unfired trigger kind); the graph is \
                              still saveable/enable-able.",
                    required: true,
                },
            ],
        },
        "get" => ControllerSchema {
            namespace: "flows",
            function: "get",
            description: "Load one saved flow by id.",
            inputs: vec![id_input("Identifier of the flow to load.")],
            outputs: vec![flow_output()],
        },
        "list" => ControllerSchema {
            namespace: "flows",
            function: "list",
            description: "List all saved flows.",
            inputs: vec![],
            outputs: vec![FieldSchema {
                name: "flows",
                ty: TypeSchema::Array(Box::new(TypeSchema::Ref("Flow"))),
                comment: "Flows currently stored in the workspace.",
                required: true,
            }],
        },
        "update" => ControllerSchema {
            namespace: "flows",
            function: "update",
            description: "Update a saved flow's name and/or graph; re-validates before persisting.",
            inputs: vec![
                id_input("Identifier of the flow to update."),
                FieldSchema {
                    name: "name",
                    ty: TypeSchema::Option(Box::new(TypeSchema::String)),
                    comment: "New name, if changing it.",
                    required: false,
                },
                FieldSchema {
                    name: "graph",
                    ty: TypeSchema::Option(Box::new(TypeSchema::Json)),
                    comment: "Replacement WorkflowGraph, if changing it.",
                    required: false,
                },
                require_approval_input(),
            ],
            outputs: vec![flow_output()],
        },
        "delete" => ControllerSchema {
            namespace: "flows",
            function: "delete",
            description: "Delete a saved flow by id.",
            inputs: vec![id_input("Identifier of the flow to delete.")],
            outputs: vec![FieldSchema {
                name: "result",
                ty: TypeSchema::Object {
                    fields: vec![
                        FieldSchema {
                            name: "id",
                            ty: TypeSchema::String,
                            comment: "Identifier that was requested for removal.",
                            required: true,
                        },
                        FieldSchema {
                            name: "removed",
                            ty: TypeSchema::Bool,
                            comment: "True when the flow was removed.",
                            required: true,
                        },
                    ],
                },
                comment: "Removal result payload.",
                required: true,
            }],
        },
        "set_enabled" => ControllerSchema {
            namespace: "flows",
            function: "set_enabled",
            description: "Enable or disable a saved flow.",
            inputs: vec![
                id_input("Identifier of the flow to toggle."),
                FieldSchema {
                    name: "enabled",
                    ty: TypeSchema::Bool,
                    comment: "New enabled state.",
                    required: true,
                },
            ],
            outputs: vec![flow_output()],
        },
        "run" => ControllerSchema {
            namespace: "flows",
            function: "run",
            description:
                "Run a saved flow to completion (or until it pauses on a human-approval gate).",
            inputs: vec![
                id_input("Identifier of the flow to run."),
                FieldSchema {
                    name: "input",
                    ty: TypeSchema::Option(Box::new(TypeSchema::Json)),
                    comment: "Trigger payload seeded into the run; defaults to null.",
                    required: false,
                },
            ],
            outputs: vec![FieldSchema {
                name: "result",
                ty: TypeSchema::Object {
                    fields: run_output_fields(),
                },
                comment: "Run outcome payload.",
                required: true,
            }],
        },
        "resume" => ControllerSchema {
            namespace: "flows",
            function: "resume",
            description: "Resume a flow run paused at a human-in-the-loop approval gate, \
                           continuing from its durable checkpoint.",
            inputs: vec![
                id_input("Identifier of the flow to resume."),
                FieldSchema {
                    name: "thread_id",
                    ty: TypeSchema::String,
                    comment:
                        "The checkpoint thread id returned by `flows_run` / a prior `flows_resume`.",
                    required: true,
                },
                FieldSchema {
                    name: "approvals",
                    ty: TypeSchema::Option(Box::new(TypeSchema::Array(Box::new(
                        TypeSchema::String,
                    )))),
                    comment: "Node ids being approved; defaults to an empty list.",
                    required: false,
                },
                FieldSchema {
                    name: "rejections",
                    ty: TypeSchema::Option(Box::new(TypeSchema::Array(Box::new(
                        TypeSchema::String,
                    )))),
                    comment: "Node ids being denied; each routes to its `error` port (or fails \
                              the run if it has none). Defaults to an empty list.",
                    required: false,
                },
            ],
            outputs: vec![FieldSchema {
                name: "result",
                ty: TypeSchema::Object {
                    fields: run_output_fields(),
                },
                comment: "Resume outcome payload (same shape as `run`'s).",
                required: true,
            }],
        },
        "cancel_run" => ControllerSchema {
            namespace: "flows",
            function: "cancel_run",
            description: "Cancel a flow run: settle it to a terminal `cancelled` status, abort \
                          the in-flight run task if one is executing, and drop its durable \
                          checkpoint so it can't be resumed.",
            inputs: vec![FieldSchema {
                name: "run_id",
                ty: TypeSchema::String,
                comment: "Identifier of the run to cancel (== its checkpoint thread id).",
                required: true,
            }],
            outputs: vec![FieldSchema {
                name: "result",
                ty: TypeSchema::Object {
                    fields: vec![
                        FieldSchema {
                            name: "run_id",
                            ty: TypeSchema::String,
                            comment: "Identifier of the run that was cancelled.",
                            required: true,
                        },
                        FieldSchema {
                            name: "cancelled",
                            ty: TypeSchema::Bool,
                            comment:
                                "True once the run is cancelled or its cancellation requested.",
                            required: true,
                        },
                        FieldSchema {
                            name: "was_in_flight",
                            ty: TypeSchema::Bool,
                            comment:
                                "True when a live run task was signalled to abort; false when \
                                      a parked/stale run row was settled directly.",
                            required: true,
                        },
                    ],
                },
                comment: "Cancellation result payload.",
                required: true,
            }],
        },
        "list_runs" => ControllerSchema {
            namespace: "flows",
            function: "list_runs",
            description: "List the most recent runs for a flow, newest first.",
            inputs: vec![
                id_input("Identifier of the flow whose runs to list."),
                FieldSchema {
                    name: "limit",
                    ty: TypeSchema::Option(Box::new(TypeSchema::U64)),
                    comment: "Maximum number of runs to return; defaults to 20.",
                    required: false,
                },
            ],
            outputs: vec![FieldSchema {
                name: "runs",
                ty: TypeSchema::Array(Box::new(TypeSchema::Ref("FlowRun"))),
                comment: "Persisted run records for this flow, newest first.",
                required: true,
            }],
        },
        "get_run" => ControllerSchema {
            namespace: "flows",
            function: "get_run",
            description: "Load one persisted flow run record by its (checkpoint thread) id.",
            inputs: vec![FieldSchema {
                name: "run_id",
                ty: TypeSchema::String,
                comment: "Identifier of the run to load (== its checkpoint thread id).",
                required: true,
            }],
            outputs: vec![FieldSchema {
                name: "run",
                ty: TypeSchema::Ref("FlowRun"),
                comment: "The persisted run record.",
                required: true,
            }],
        },
        _other => ControllerSchema {
            namespace: "flows",
            function: "unknown",
            description: "Unknown flows controller function.",
            inputs: vec![FieldSchema {
                name: "function",
                ty: TypeSchema::String,
                comment: "Unknown function requested for schema lookup.",
                required: true,
            }],
            outputs: vec![FieldSchema {
                name: "error",
                ty: TypeSchema::String,
                comment: "Lookup error details.",
                required: true,
            }],
        },
    }
}

fn handle_create(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout().await?;
        let name = read_required::<String>(&params, "name")?;
        let graph = read_required::<Value>(&params, "graph")?;
        let require_approval = params
            .get("require_approval")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        to_json(ops::flows_create(&config, name, graph, require_approval).await?)
    })
}

fn handle_validate(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        // No config load: validation is pure (no persistence, no workspace).
        let graph = read_required::<Value>(&params, "graph")?;
        to_json(ops::flows_validate(graph))
    })
}

fn handle_get(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout().await?;
        let id = read_required::<String>(&params, "id")?;
        to_json(ops::flows_get(&config, id.trim()).await?)
    })
}

fn handle_list(_params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout().await?;
        to_json(ops::flows_list(&config).await?)
    })
}

fn handle_update(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout().await?;
        let id = read_required::<String>(&params, "id")?;
        let name = params
            .get("name")
            .filter(|v| !v.is_null())
            .map(|v| serde_json::from_value(v.clone()))
            .transpose()
            .map_err(|e| format!("invalid 'name': {e}"))?;
        let graph = params.get("graph").filter(|v| !v.is_null()).cloned();
        let require_approval = params.get("require_approval").and_then(Value::as_bool);
        to_json(ops::flows_update(&config, id.trim(), name, graph, require_approval).await?)
    })
}

fn handle_delete(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout().await?;
        let id = read_required::<String>(&params, "id")?;
        to_json(ops::flows_delete(&config, id.trim()).await?)
    })
}

fn handle_set_enabled(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout().await?;
        let id = read_required::<String>(&params, "id")?;
        let enabled = params
            .get("enabled")
            .and_then(Value::as_bool)
            .ok_or_else(|| "missing required param 'enabled'".to_string())?;
        to_json(ops::flows_set_enabled(&config, id.trim(), enabled).await?)
    })
}

fn handle_run(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout().await?;
        let id = read_required::<String>(&params, "id")?;
        let input = params.get("input").cloned().unwrap_or(Value::Null);
        to_json(
            ops::flows_run(
                &config,
                id.trim(),
                input,
                crate::openhuman::flows::FlowRunTrigger::Rpc,
            )
            .await?,
        )
    })
}

fn handle_resume(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout().await?;
        let id = read_required::<String>(&params, "id")?;
        let thread_id = read_required::<String>(&params, "thread_id")?;
        let approvals: Vec<String> = params
            .get("approvals")
            .filter(|v| !v.is_null())
            .cloned()
            .map(serde_json::from_value)
            .transpose()
            .map_err(|e| format!("invalid 'approvals': {e}"))?
            .unwrap_or_default();
        let rejections: Vec<String> = params
            .get("rejections")
            .filter(|v| !v.is_null())
            .cloned()
            .map(serde_json::from_value)
            .transpose()
            .map_err(|e| format!("invalid 'rejections': {e}"))?
            .unwrap_or_default();
        to_json(
            ops::flows_resume(&config, id.trim(), thread_id.trim(), approvals, rejections).await?,
        )
    })
}

fn handle_cancel_run(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout().await?;
        let run_id = read_required::<String>(&params, "run_id")?;
        to_json(ops::flows_cancel_run(&config, run_id.trim()).await?)
    })
}

fn handle_list_runs(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout().await?;
        let id = read_required::<String>(&params, "id")?;
        let limit = params
            .get("limit")
            .and_then(Value::as_u64)
            .and_then(|n| usize::try_from(n).ok())
            .unwrap_or(20);
        to_json(ops::flows_list_runs(&config, id.trim(), limit).await?)
    })
}

fn handle_get_run(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout().await?;
        let run_id = read_required::<String>(&params, "run_id")?;
        to_json(ops::flows_get_run(&config, run_id.trim()).await?)
    })
}

fn read_required<T: DeserializeOwned>(params: &Map<String, Value>, key: &str) -> Result<T, String> {
    let value = params
        .get(key)
        .cloned()
        .ok_or_else(|| format!("missing required param '{key}'"))?;
    serde_json::from_value(value).map_err(|e| format!("invalid '{key}': {e}"))
}

fn to_json<T: serde::Serialize>(outcome: RpcOutcome<T>) -> Result<Value, String> {
    outcome.into_cli_compatible_json()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_controller_schemas_covers_every_supported_function() {
        let names: Vec<_> = all_controller_schemas()
            .into_iter()
            .map(|s| s.function)
            .collect();
        assert_eq!(
            names,
            vec![
                "create",
                "validate",
                "get",
                "list",
                "update",
                "delete",
                "set_enabled",
                "run",
                "resume",
                "cancel_run",
                "list_runs",
                "get_run",
            ]
        );
    }

    #[test]
    fn all_registered_controllers_has_handler_per_schema() {
        let controllers = all_registered_controllers();
        assert_eq!(controllers.len(), 12);
        let names: Vec<_> = controllers.iter().map(|c| c.schema.function).collect();
        assert_eq!(
            names,
            vec![
                "create",
                "validate",
                "get",
                "list",
                "update",
                "delete",
                "set_enabled",
                "run",
                "resume",
                "cancel_run",
                "list_runs",
                "get_run",
            ]
        );
    }

    #[test]
    fn schemas_create_requires_name_and_graph() {
        let s = schemas("create");
        assert_eq!(s.namespace, "flows");
        let required: Vec<_> = s
            .inputs
            .iter()
            .filter(|f| f.required)
            .map(|f| f.name)
            .collect();
        assert_eq!(required, vec!["name", "graph"]);
    }

    #[test]
    fn schemas_create_require_approval_is_optional() {
        let s = schemas("create");
        let field = s
            .inputs
            .iter()
            .find(|f| f.name == "require_approval")
            .unwrap();
        assert!(!field.required);
    }

    #[test]
    fn schemas_run_input_is_optional() {
        let s = schemas("run");
        let input = s.inputs.iter().find(|f| f.name == "input").unwrap();
        assert!(!input.required);
    }

    #[test]
    fn schemas_resume_requires_id_and_thread_id_but_not_approvals() {
        let s = schemas("resume");
        let required: Vec<_> = s
            .inputs
            .iter()
            .filter(|f| f.required)
            .map(|f| f.name)
            .collect();
        assert_eq!(required, vec!["id", "thread_id"]);
        let approvals = s.inputs.iter().find(|f| f.name == "approvals").unwrap();
        assert!(!approvals.required);
    }

    #[test]
    fn schemas_list_runs_limit_is_optional() {
        let s = schemas("list_runs");
        let limit = s.inputs.iter().find(|f| f.name == "limit").unwrap();
        assert!(!limit.required);
    }

    #[test]
    fn schemas_get_run_requires_run_id() {
        let s = schemas("get_run");
        let required: Vec<_> = s
            .inputs
            .iter()
            .filter(|f| f.required)
            .map(|f| f.name)
            .collect();
        assert_eq!(required, vec!["run_id"]);
    }

    #[test]
    fn schemas_unknown_function_returns_placeholder() {
        let s = schemas("does-not-exist");
        assert_eq!(s.function, "unknown");
        assert_eq!(s.outputs[0].name, "error");
    }

    #[test]
    fn read_required_errors_when_missing() {
        let params = Map::new();
        let err = read_required::<String>(&params, "id").unwrap_err();
        assert!(err.contains("missing required param 'id'"));
    }
}
