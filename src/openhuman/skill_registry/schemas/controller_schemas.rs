//! Controller schema definitions for `openhuman.skill_registry_*` RPC methods.

use crate::core::all::RegisteredController;
use crate::core::{ControllerSchema, FieldSchema, TypeSchema};

use super::handlers::{
    handle_add_source, handle_browse, handle_install, handle_remove_source, handle_search,
    handle_sources,
};

pub fn all_skill_registry_controller_schemas() -> Vec<ControllerSchema> {
    vec![
        skill_registry_schemas("browse"),
        skill_registry_schemas("search"),
        skill_registry_schemas("sources"),
        skill_registry_schemas("add_source"),
        skill_registry_schemas("remove_source"),
        skill_registry_schemas("install"),
    ]
}

pub fn all_skill_registry_registered_controllers() -> Vec<RegisteredController> {
    vec![
        RegisteredController {
            schema: skill_registry_schemas("browse"),
            handler: handle_browse,
        },
        RegisteredController {
            schema: skill_registry_schemas("search"),
            handler: handle_search,
        },
        RegisteredController {
            schema: skill_registry_schemas("sources"),
            handler: handle_sources,
        },
        RegisteredController {
            schema: skill_registry_schemas("add_source"),
            handler: handle_add_source,
        },
        RegisteredController {
            schema: skill_registry_schemas("remove_source"),
            handler: handle_remove_source,
        },
        RegisteredController {
            schema: skill_registry_schemas("install"),
            handler: handle_install,
        },
    ]
}

pub fn skill_registry_schemas(function: &str) -> ControllerSchema {
    match function {
        "browse" => ControllerSchema {
            namespace: "skill_registry",
            function: "browse",
            description: "Browse the skill registry catalog from all enabled sources. Returns cached results unless force_refresh is true.",
            inputs: vec![FieldSchema {
                name: "force_refresh",
                ty: TypeSchema::Bool,
                comment: "Force re-fetch from remote sources, ignoring the local cache.",
                required: false,
            }],
            outputs: vec![FieldSchema {
                name: "entries",
                ty: TypeSchema::Json,
                comment: "Array of catalog entries from all enabled registry sources.",
                required: true,
            }],
        },
        "search" => ControllerSchema {
            namespace: "skill_registry",
            function: "search",
            description: "Search the registry catalog by query string. Matches against name, description, tags, format, and author.",
            inputs: vec![
                FieldSchema {
                    name: "query",
                    ty: TypeSchema::String,
                    comment: "Search query string.",
                    required: false,
                },
                FieldSchema {
                    name: "format",
                    ty: TypeSchema::String,
                    comment: "Filter by skill format: openhuman, hermes, or openclaw.",
                    required: false,
                },
                FieldSchema {
                    name: "source",
                    ty: TypeSchema::String,
                    comment: "Filter by source id.",
                    required: false,
                },
            ],
            outputs: vec![FieldSchema {
                name: "entries",
                ty: TypeSchema::Json,
                comment: "Matching catalog entries.",
                required: true,
            }],
        },
        "sources" => ControllerSchema {
            namespace: "skill_registry",
            function: "sources",
            description: "List all configured registry sources (default + custom).",
            inputs: vec![],
            outputs: vec![FieldSchema {
                name: "sources",
                ty: TypeSchema::Json,
                comment: "Array of registry sources with id, name, url, kind, and enabled status.",
                required: true,
            }],
        },
        "add_source" => ControllerSchema {
            namespace: "skill_registry",
            function: "add_source",
            description: "Add a custom registry source. Clears the catalog cache.",
            inputs: vec![
                FieldSchema {
                    name: "id",
                    ty: TypeSchema::String,
                    comment: "Unique identifier for the source.",
                    required: true,
                },
                FieldSchema {
                    name: "name",
                    ty: TypeSchema::String,
                    comment: "Display name.",
                    required: true,
                },
                FieldSchema {
                    name: "url",
                    ty: TypeSchema::String,
                    comment: "URL to the index.json catalog.",
                    required: true,
                },
                FieldSchema {
                    name: "kind",
                    ty: TypeSchema::String,
                    comment: "Registry kind: github_index or http_catalog. Default: github_index.",
                    required: false,
                },
            ],
            outputs: vec![FieldSchema {
                name: "sources",
                ty: TypeSchema::Json,
                comment: "Updated list of all sources.",
                required: true,
            }],
        },
        "remove_source" => ControllerSchema {
            namespace: "skill_registry",
            function: "remove_source",
            description: "Remove a custom registry source by id. Default sources cannot be removed.",
            inputs: vec![FieldSchema {
                name: "id",
                ty: TypeSchema::String,
                comment: "Id of the custom source to remove.",
                required: true,
            }],
            outputs: vec![FieldSchema {
                name: "sources",
                ty: TypeSchema::Json,
                comment: "Updated list of all sources.",
                required: true,
            }],
        },
        "install" => ControllerSchema {
            namespace: "skill_registry",
            function: "install",
            description: "Install a skill from the registry by its catalog entry id and source id. Fetches the SKILL.md and installs to user scope.",
            inputs: vec![
                FieldSchema {
                    name: "entry_id",
                    ty: TypeSchema::String,
                    comment: "Catalog entry id of the skill to install.",
                    required: true,
                },
                FieldSchema {
                    name: "source_id",
                    ty: TypeSchema::String,
                    comment: "Registry source id the entry belongs to.",
                    required: true,
                },
            ],
            outputs: vec![
                FieldSchema {
                    name: "url",
                    ty: TypeSchema::String,
                    comment: "The URL that was fetched.",
                    required: true,
                },
                FieldSchema {
                    name: "stdout",
                    ty: TypeSchema::String,
                    comment: "Diagnostic summary.",
                    required: true,
                },
                FieldSchema {
                    name: "stderr",
                    ty: TypeSchema::String,
                    comment: "Parse warnings.",
                    required: true,
                },
                FieldSchema {
                    name: "new_skills",
                    ty: TypeSchema::Array(Box::new(TypeSchema::String)),
                    comment: "Slugs of skills that appeared post-install.",
                    required: true,
                },
            ],
        },
        _ => ControllerSchema {
            namespace: "skill_registry",
            function: "unknown",
            description: "Unknown skill_registry controller.",
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
