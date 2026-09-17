//! Schemas and handlers for the source-registry CRUD controllers: list, get,
//! add, update, remove, list_items, read_item.

use serde_json::{Map, Value};

use crate::core::all::ControllerFuture;
use crate::core::{ControllerSchema, FieldSchema, TypeSchema};
use crate::memory::sources::rpc;

use super::{parse_value, to_json, NAMESPACE};

pub(super) fn kind_specific_fields() -> Vec<FieldSchema> {
    vec![
        FieldSchema {
            name: "toolkit",
            ty: TypeSchema::Option(Box::new(TypeSchema::String)),
            comment: "Composio toolkit slug.",
            required: false,
        },
        FieldSchema {
            name: "connection_id",
            ty: TypeSchema::Option(Box::new(TypeSchema::String)),
            comment: "Composio connection id.",
            required: false,
        },
        FieldSchema {
            name: "path",
            ty: TypeSchema::Option(Box::new(TypeSchema::String)),
            comment: "Local folder path.",
            required: false,
        },
        FieldSchema {
            name: "glob",
            ty: TypeSchema::Option(Box::new(TypeSchema::String)),
            comment: "Glob pattern for folder sources.",
            required: false,
        },
        FieldSchema {
            name: "url",
            ty: TypeSchema::Option(Box::new(TypeSchema::String)),
            comment: "URL for github_repo, rss_feed, or web_page sources.",
            required: false,
        },
        FieldSchema {
            name: "branch",
            ty: TypeSchema::Option(Box::new(TypeSchema::String)),
            comment: "Git branch for github_repo sources.",
            required: false,
        },
        FieldSchema {
            name: "paths",
            ty: TypeSchema::Array(Box::new(TypeSchema::String)),
            comment: "Path filters for github_repo sources.",
            required: false,
        },
        FieldSchema {
            name: "max_commits",
            ty: TypeSchema::Option(Box::new(TypeSchema::U64)),
            comment: "Max commits per sync for github_repo sources.",
            required: false,
        },
        FieldSchema {
            name: "max_issues",
            ty: TypeSchema::Option(Box::new(TypeSchema::U64)),
            comment: "Max issues per sync for github_repo sources.",
            required: false,
        },
        FieldSchema {
            name: "max_prs",
            ty: TypeSchema::Option(Box::new(TypeSchema::U64)),
            comment: "Max pull requests per sync for github_repo sources.",
            required: false,
        },
        FieldSchema {
            name: "query",
            ty: TypeSchema::Option(Box::new(TypeSchema::String)),
            comment: "Search query for twitter_query sources.",
            required: false,
        },
        FieldSchema {
            name: "since_days",
            ty: TypeSchema::Option(Box::new(TypeSchema::U64)),
            comment: "Lookback window in days for twitter_query.",
            required: false,
        },
        FieldSchema {
            name: "max_items",
            ty: TypeSchema::Option(Box::new(TypeSchema::U64)),
            comment: "Maximum items for rss_feed or composio sources.",
            required: false,
        },
        FieldSchema {
            name: "selector",
            ty: TypeSchema::Option(Box::new(TypeSchema::String)),
            comment: "CSS selector for web_page sources.",
            required: false,
        },
        FieldSchema {
            name: "max_tokens_per_sync",
            ty: TypeSchema::Option(Box::new(TypeSchema::U64)),
            comment: "Max tokens per sync run.",
            required: false,
        },
        FieldSchema {
            name: "max_cost_per_sync_usd",
            ty: TypeSchema::Option(Box::new(TypeSchema::F64)),
            comment: "Max cost per sync run in USD.",
            required: false,
        },
        FieldSchema {
            name: "sync_depth_days",
            ty: TypeSchema::Option(Box::new(TypeSchema::U64)),
            comment: "Only sync items from the last N days.",
            required: false,
        },
    ]
}

pub(super) fn schema(function: &str) -> Option<ControllerSchema> {
    Some(match function {
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
        "add" => {
            let mut inputs = vec![
                FieldSchema {
                    name: "kind",
                    ty: TypeSchema::Enum {
                        variants: vec![
                            "composio",
                            "conversation",
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
            ];
            inputs.extend(kind_specific_fields());
            ControllerSchema {
                namespace: NAMESPACE,
                function: "add",
                description:
                    "Add a new memory source. Kind-specific fields are flat on the request.",
                inputs,
                outputs: vec![FieldSchema {
                    name: "source",
                    ty: TypeSchema::Ref("MemorySourceEntry"),
                    comment: "The newly created source.",
                    required: true,
                }],
            }
        }
        "update" => {
            let mut inputs = vec![
                FieldSchema {
                    name: "id",
                    ty: TypeSchema::String,
                    comment: "Source id to update.",
                    required: true,
                },
                FieldSchema {
                    name: "label",
                    ty: TypeSchema::Option(Box::new(TypeSchema::String)),
                    comment: "New label.",
                    required: false,
                },
                FieldSchema {
                    name: "enabled",
                    ty: TypeSchema::Option(Box::new(TypeSchema::Bool)),
                    comment: "Enable or disable.",
                    required: false,
                },
            ];
            inputs.extend(kind_specific_fields());
            ControllerSchema {
                namespace: NAMESPACE,
                function: "update",
                description: "Partial update of a memory source.",
                inputs,
                outputs: vec![FieldSchema {
                    name: "source",
                    ty: TypeSchema::Ref("MemorySourceEntry"),
                    comment: "The updated source.",
                    required: true,
                }],
            }
        }
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
        _ => return None,
    })
}

pub(super) fn handle_list(_params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move { to_json(rpc::list_rpc().await?) })
}

pub(super) fn handle_get(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let req = parse_value::<rpc::GetRequest>(Value::Object(params))?;
        to_json(rpc::get_rpc(req).await?)
    })
}

pub(super) fn handle_add(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let req = parse_value::<rpc::AddRequest>(Value::Object(params))?;
        to_json(rpc::add_rpc(req).await?)
    })
}

pub(super) fn handle_update(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let req = parse_value::<rpc::UpdateRequest>(Value::Object(params))?;
        to_json(rpc::update_rpc(req).await?)
    })
}

pub(super) fn handle_remove(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let req = parse_value::<rpc::RemoveRequest>(Value::Object(params))?;
        to_json(rpc::remove_rpc(req).await?)
    })
}

pub(super) fn handle_list_items(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let req = parse_value::<rpc::ListItemsRequest>(Value::Object(params))?;
        to_json(rpc::list_items_rpc(req).await?)
    })
}

pub(super) fn handle_read_item(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let req = parse_value::<rpc::ReadItemRequest>(Value::Object(params))?;
        to_json(rpc::read_item_rpc(req).await?)
    })
}
