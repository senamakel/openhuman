//! Controller-registry schemas for `openhuman.memory_sources_*`.

use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

use crate::core::all::{ControllerFuture, RegisteredController};
use crate::core::{ControllerSchema, FieldSchema, TypeSchema};
use crate::rpc::RpcOutcome;

use super::rpc;

const NAMESPACE: &str = "memory_sources";

pub fn all_controller_schemas() -> Vec<ControllerSchema> {
    vec![
        schemas("list"),
        schemas("get"),
        schemas("add"),
        schemas("update"),
        schemas("remove"),
        schemas("list_items"),
        schemas("read_item"),
    ]
}

pub fn all_registered_controllers() -> Vec<RegisteredController> {
    vec![
        RegisteredController {
            schema: schemas("list"),
            handler: handle_list,
        },
        RegisteredController {
            schema: schemas("get"),
            handler: handle_get,
        },
        RegisteredController {
            schema: schemas("add"),
            handler: handle_add,
        },
        RegisteredController {
            schema: schemas("update"),
            handler: handle_update,
        },
        RegisteredController {
            schema: schemas("remove"),
            handler: handle_remove,
        },
        RegisteredController {
            schema: schemas("list_items"),
            handler: handle_list_items,
        },
        RegisteredController {
            schema: schemas("read_item"),
            handler: handle_read_item,
        },
    ]
}

pub fn schemas(function: &str) -> ControllerSchema {
    match function {
        "list" => ControllerSchema {
            namespace: NAMESPACE,
            function: "list",
            description: "List all configured memory sources.",
            inputs: vec![],
            outputs: vec![FieldSchema {
                name: "sources",
                ty: TypeSchema::Array(Box::new(TypeSchema::Ref("MemorySourceEntry"))),
                comment: "All configured sources.",
                required: true,
            }],
        },
        "get" => ControllerSchema {
            namespace: NAMESPACE,
            function: "get",
            description: "Get a single memory source by id.",
            inputs: vec![FieldSchema {
                name: "id",
                ty: TypeSchema::String,
                comment: "Source id.",
                required: true,
            }],
            outputs: vec![FieldSchema {
                name: "source",
                ty: TypeSchema::Option(Box::new(TypeSchema::Ref("MemorySourceEntry"))),
                comment: "The source if found.",
                required: false,
            }],
        },
        "add" => ControllerSchema {
            namespace: NAMESPACE,
            function: "add",
            description: "Add a new memory source. Kind-specific fields are flat on the request.",
            inputs: vec![
                FieldSchema {
                    name: "kind",
                    ty: TypeSchema::Enum {
                        variants: vec![
                            "composio",
                            "folder",
                            "github_repo",
                            "twitter_query",
                            "rss_feed",
                            "web_page",
                        ],
                    },
                    comment: "Source kind.",
                    required: true,
                },
                FieldSchema {
                    name: "label",
                    ty: TypeSchema::String,
                    comment: "User-facing display name.",
                    required: true,
                },
                FieldSchema {
                    name: "enabled",
                    ty: TypeSchema::Bool,
                    comment: "Whether the source is active. Defaults to true.",
                    required: false,
                },
            ],
            outputs: vec![FieldSchema {
                name: "source",
                ty: TypeSchema::Ref("MemorySourceEntry"),
                comment: "The newly created source.",
                required: true,
            }],
        },
        "update" => ControllerSchema {
            namespace: NAMESPACE,
            function: "update",
            description: "Partial update of a memory source.",
            inputs: vec![FieldSchema {
                name: "id",
                ty: TypeSchema::String,
                comment: "Source id to update.",
                required: true,
            }],
            outputs: vec![FieldSchema {
                name: "source",
                ty: TypeSchema::Ref("MemorySourceEntry"),
                comment: "The updated source.",
                required: true,
            }],
        },
        "remove" => ControllerSchema {
            namespace: NAMESPACE,
            function: "remove",
            description: "Remove a memory source.",
            inputs: vec![FieldSchema {
                name: "id",
                ty: TypeSchema::String,
                comment: "Source id to remove.",
                required: true,
            }],
            outputs: vec![FieldSchema {
                name: "removed",
                ty: TypeSchema::Bool,
                comment: "True if the source was found and removed.",
                required: true,
            }],
        },
        "list_items" => ControllerSchema {
            namespace: NAMESPACE,
            function: "list_items",
            description: "List readable items from a memory source via its reader.",
            inputs: vec![FieldSchema {
                name: "source_id",
                ty: TypeSchema::String,
                comment: "Source id to list items from.",
                required: true,
            }],
            outputs: vec![FieldSchema {
                name: "items",
                ty: TypeSchema::Array(Box::new(TypeSchema::Ref("SourceItem"))),
                comment: "Items available in the source.",
                required: true,
            }],
        },
        "read_item" => ControllerSchema {
            namespace: NAMESPACE,
            function: "read_item",
            description: "Read one item's content from a memory source.",
            inputs: vec![
                FieldSchema {
                    name: "source_id",
                    ty: TypeSchema::String,
                    comment: "Source id.",
                    required: true,
                },
                FieldSchema {
                    name: "item_id",
                    ty: TypeSchema::String,
                    comment: "Item id within the source.",
                    required: true,
                },
            ],
            outputs: vec![FieldSchema {
                name: "content",
                ty: TypeSchema::Ref("SourceContent"),
                comment: "The item's content.",
                required: true,
            }],
        },
        other => panic!("unknown memory_sources schema function: {other}"),
    }
}

fn handle_list(_params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move { to_json(rpc::list_rpc().await?) })
}

fn handle_get(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let req = parse_value::<rpc::GetRequest>(Value::Object(params))?;
        to_json(rpc::get_rpc(req).await?)
    })
}

fn handle_add(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let req = parse_value::<rpc::AddRequest>(Value::Object(params))?;
        to_json(rpc::add_rpc(req).await?)
    })
}

fn handle_update(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let req = parse_value::<rpc::UpdateRequest>(Value::Object(params))?;
        to_json(rpc::update_rpc(req).await?)
    })
}

fn handle_remove(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let req = parse_value::<rpc::RemoveRequest>(Value::Object(params))?;
        to_json(rpc::remove_rpc(req).await?)
    })
}

fn handle_list_items(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let req = parse_value::<rpc::ListItemsRequest>(Value::Object(params))?;
        to_json(rpc::list_items_rpc(req).await?)
    })
}

fn handle_read_item(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let req = parse_value::<rpc::ReadItemRequest>(Value::Object(params))?;
        to_json(rpc::read_item_rpc(req).await?)
    })
}

fn parse_value<T: DeserializeOwned>(v: Value) -> Result<T, String> {
    serde_json::from_value(v).map_err(|e| format!("invalid params: {e}"))
}

fn to_json<T: serde::Serialize>(outcome: RpcOutcome<T>) -> Result<Value, String> {
    outcome.into_cli_compatible_json()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_controller_schemas_and_registered_controllers_stay_in_sync() {
        let schemas = all_controller_schemas();
        let controllers = all_registered_controllers();
        assert_eq!(schemas.len(), controllers.len());
        assert!(schemas.iter().all(|s| s.namespace == NAMESPACE));
    }

    #[test]
    #[should_panic(expected = "unknown memory_sources schema function")]
    fn schemas_panics_on_unknown_function() {
        schemas("nope");
    }
}
