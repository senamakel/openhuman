//! The `modules` RPC namespace.
//!
//! Read-only plus one deliberate action. `list` and `status` report what this
//! build knows and what it has loaded; `load` forces a lazy module to resolve
//! now, which is what a settings screen offering "install this now" needs.
//!
//! There is no `unload`, and there cannot be: tinybus never unloads a library.
//! There is also no way to name an artifact over RPC — the loadable set is
//! compiled into [`super::registry`], and a method that could point the loader at
//! an arbitrary path would turn this namespace into remote code execution.

use crate::core::runtime::DispatchCtx;
use serde_json::{Map, Value};

use super::ops;
use crate::core::all::{ControllerFuture, RegisteredController};
use crate::core::{ControllerSchema, FieldSchema, TypeSchema};

pub fn all_controller_schemas() -> Vec<ControllerSchema> {
    vec![
        schemas("list"),
        schemas("status"),
        schemas("load"),
        schemas("browser_check_readiness"),
        schemas("browser_forget_sites"),
        schemas("computer_status"),
    ]
}

pub fn all_registered_controllers() -> Vec<RegisteredController> {
    vec![
        RegisteredController {
            schema: schemas("list"),
            handler: handle_list,
        },
        RegisteredController {
            schema: schemas("status"),
            handler: handle_status,
        },
        RegisteredController {
            schema: schemas("load"),
            handler: handle_load,
        },
        RegisteredController {
            schema: schemas("browser_check_readiness"),
            handler: crate::desktop::computer_rpc::handle_browser_check_readiness,
        },
        RegisteredController {
            schema: schemas("browser_forget_sites"),
            handler: crate::desktop::computer_rpc::handle_browser_forget_sites,
        },
        RegisteredController {
            schema: schemas("computer_status"),
            handler: crate::desktop::computer_rpc::handle_computer_status,
        },
    ]
}

pub fn schemas(function: &str) -> ControllerSchema {
    match function {
        "list" => ControllerSchema {
            namespace: "modules",
            function: "list",
            description: "List every loadable module this build knows, with its state.",
            inputs: vec![],
            outputs: vec![FieldSchema {
                name: "modules",
                ty: TypeSchema::Array(Box::new(TypeSchema::Ref("ModuleStatus"))),
                comment: "Status of each known module.",
                required: true,
            }],
        },
        "status" => ControllerSchema {
            namespace: "modules",
            function: "status",
            description: "Report the state of one module by id.",
            inputs: vec![FieldSchema {
                name: "id",
                ty: TypeSchema::String,
                comment: "Registry identifier, e.g. `tinydocs`.",
                required: true,
            }],
            outputs: vec![FieldSchema {
                name: "module",
                ty: TypeSchema::Ref("ModuleStatus"),
                comment: "Status of the requested module.",
                required: true,
            }],
        },
        "load" => ControllerSchema {
            namespace: "modules",
            function: "load",
            description: "Resolve and load a module now instead of on first use.",
            inputs: vec![FieldSchema {
                name: "id",
                ty: TypeSchema::String,
                comment: "Registry identifier, e.g. `tinydocs`.",
                required: true,
            }],
            outputs: vec![FieldSchema {
                name: "module",
                ty: TypeSchema::Ref("ModuleStatus"),
                comment: "Status after the load attempt.",
                required: true,
            }],
        },
        "browser_check_readiness" => ControllerSchema {
            namespace: "modules",
            function: "browser_check_readiness",
            description: "Load TinyComputer and briefly launch Chrome to check browser readiness.",
            inputs: vec![],
            outputs: vec![
                FieldSchema {
                    name: "module_ready",
                    ty: TypeSchema::Bool,
                    comment: "TinyComputer module is serving its browser members.",
                    required: true,
                },
                FieldSchema {
                    name: "chrome_ready",
                    ty: TypeSchema::Bool,
                    comment: "Chrome launched and closed successfully.",
                    required: true,
                },
                FieldSchema {
                    name: "error",
                    ty: TypeSchema::Option(Box::new(TypeSchema::String)),
                    comment: "Sanitized setup failure, if any.",
                    required: false,
                },
            ],
        },
        "browser_forget_sites" => ControllerSchema {
            namespace: "modules",
            function: "browser_forget_sites",
            description: "Forget what finished browser tasks learned about one site, or about every site.",
            inputs: vec![FieldSchema {
                name: "site",
                ty: TypeSchema::Option(Box::new(TypeSchema::String)),
                comment: "The site's host or an address on it; omit to forget every site.",
                required: false,
            }],
            outputs: vec![FieldSchema {
                name: "forgotten",
                ty: TypeSchema::U64,
                comment: "How many sites were forgotten.",
                required: true,
            }],
        },
        "computer_status" => ControllerSchema {
            namespace: "modules",
            function: "computer_status",
            description: "Report the TinyComputer module, its decision and planner routes, and optionally what it says is configured.",
            inputs: vec![FieldSchema {
                name: "load",
                ty: TypeSchema::Option(Box::new(TypeSchema::Bool)),
                comment: "Load the module and ask it (Describe) even when it is not serving yet.",
                required: false,
            }],
            outputs: vec![FieldSchema {
                name: "status",
                ty: TypeSchema::Json,
                comment: "Module status, decision_model, decision_route, planner_route, and capabilities or error.",
                required: true,
            }],
        },
        _ => ControllerSchema {
            namespace: "modules",
            function: "unknown",
            description: "Unknown modules controller function.",
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

fn handle_list(ctx: DispatchCtx, _params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = ctx.load_config().await?;
        Ok(serde_json::json!({ "modules": ops::list(&config) }))
    })
}

fn handle_status(ctx: DispatchCtx, params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let id = string_param(&params, "id").ok_or("`id` is required")?;
        let config = ctx.load_config().await?;
        match ops::list(&config).into_iter().find(|m| m.id == id) {
            Some(module) => Ok(serde_json::json!({ "module": module })),
            None => Err(format!("unknown module '{id}'")),
        }
    })
}

fn handle_load(ctx: DispatchCtx, params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let id = string_param(&params, "id").ok_or("`id` is required")?;
        let config = ctx.load_config().await?;
        // A failed load is reported through the returned status rather than as
        // an RPC error: the caller asked "what happened", and the answer is a
        // state plus a reason, not a transport failure.
        if let Err(reason) = ops::ensure_loaded(&config, &id).await {
            log::warn!("[modules] explicit load of '{id}' failed: {reason}");
        }
        match ops::list(&config).into_iter().find(|m| m.id == id) {
            Some(module) => Ok(serde_json::json!({ "module": module })),
            None => Err(format!("unknown module '{id}'")),
        }
    })
}

/// A required string parameter, rejecting a blank one.
fn string_param(params: &Map<String, Value>, key: &str) -> Option<String> {
    params
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
#[path = "schemas_tests.rs"]
mod tests;
