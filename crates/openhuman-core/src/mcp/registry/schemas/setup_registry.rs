//! All setup-agent schemas under the `mcp_setup` RPC namespace. Kept
//! separate from `registry.rs` so the setup surface can evolve
//! independently of the existing `mcp_clients_*` controllers.

use crate::core::{ControllerSchema, FieldSchema, TypeSchema};

pub(crate) fn setup_schemas(function: &str) -> ControllerSchema {
    match function {
        "search" => ControllerSchema {
            namespace: "mcp_setup",
            function: "search",
            description: "Search all enabled MCP registries (Smithery + official).",
            inputs: vec![
                FieldSchema {
                    name: "query",
                    ty: TypeSchema::Option(Box::new(TypeSchema::String)),
                    comment: "Free-text search query.",
                    required: false,
                },
                FieldSchema {
                    name: "page",
                    ty: TypeSchema::Option(Box::new(TypeSchema::U64)),
                    comment: "1-based page number (default: 1).",
                    required: false,
                },
                FieldSchema {
                    name: "page_size",
                    ty: TypeSchema::Option(Box::new(TypeSchema::U64)),
                    comment: "Results per page (default: 20).",
                    required: false,
                },
            ],
            outputs: vec![
                FieldSchema {
                    name: "servers",
                    ty: TypeSchema::Array(Box::new(TypeSchema::Ref("SmitheryServerSummary"))),
                    comment: "Merged summaries; each row tagged with its `source` (`smithery` | `mcp_official`).",
                    required: true,
                },
                FieldSchema {
                    name: "page",
                    ty: TypeSchema::U64,
                    comment: "Current page number.",
                    required: true,
                },
                FieldSchema {
                    name: "total_pages",
                    ty: TypeSchema::U64,
                    comment: "Upper-bound page count across registries.",
                    required: true,
                },
            ],
        },
        "get" => ControllerSchema {
            namespace: "mcp_setup",
            function: "get",
            description: "Fetch full details for one server. Adds `required_env_keys` derived from the connection schema.",
            inputs: vec![FieldSchema {
                name: "qualified_name",
                ty: TypeSchema::String,
                comment: "Registry qualified name. May be prefixed with `<source>::` to pin a registry.",
                required: true,
            }],
            outputs: vec![FieldSchema {
                name: "server",
                ty: TypeSchema::Ref("SmitheryServerDetail"),
                comment: "Full detail with `required_env_keys` injected.",
                required: true,
            }],
        },
        "request_secret" => ControllerSchema {
            namespace: "mcp_setup",
            function: "request_secret",
            description: "Ask the user out-of-band for a secret value. Blocks until the UI submits via `submit_secret` (5-minute timeout). Returns an opaque ref; the raw value never enters the agent's context.",
            inputs: vec![
                FieldSchema {
                    name: "key_name",
                    ty: TypeSchema::String,
                    comment: "Display name of the env var (e.g. `NOTION_API_KEY`).",
                    required: true,
                },
                FieldSchema {
                    name: "prompt",
                    ty: TypeSchema::String,
                    comment: "Plain-English instruction shown to the user in the native input box.",
                    required: true,
                },
            ],
            outputs: vec![
                FieldSchema {
                    name: "ref",
                    ty: TypeSchema::String,
                    comment: "Opaque handle like `secret://<hex>`. Pass back via `test_connection` / `install_and_connect`.",
                    required: true,
                },
                FieldSchema {
                    name: "key_name",
                    ty: TypeSchema::String,
                    comment: "Echoed key name.",
                    required: true,
                },
            ],
        },
        "submit_secret" => ControllerSchema {
            namespace: "mcp_setup",
            function: "submit_secret",
            description: "UI-side: fulfill a pending `request_secret` with the user-entered value. Not intended for agent use.",
            inputs: vec![
                FieldSchema {
                    name: "ref_id",
                    ty: TypeSchema::String,
                    comment: "The `secret://<hex>` ref returned by `request_secret`.",
                    required: true,
                },
                FieldSchema {
                    name: "value",
                    ty: TypeSchema::String,
                    comment: "Raw secret value. NEVER log this.",
                    required: true,
                },
            ],
            outputs: vec![
                FieldSchema {
                    name: "ref",
                    ty: TypeSchema::String,
                    comment: "Echoed ref.",
                    required: true,
                },
                FieldSchema {
                    name: "fulfilled",
                    ty: TypeSchema::Bool,
                    comment: "True on success.",
                    required: true,
                },
            ],
        },
        "test_connection" => ControllerSchema {
            namespace: "mcp_setup",
            function: "test_connection",
            description: "Dry-run install: spawn a candidate server in a scratch process with the supplied secret refs, list its tools, tear down. Nothing persisted.",
            inputs: vec![
                FieldSchema {
                    name: "qualified_name",
                    ty: TypeSchema::String,
                    comment: "Registry qualified name.",
                    required: true,
                },
                FieldSchema {
                    name: "env_refs",
                    ty: TypeSchema::Map(Box::new(TypeSchema::String)),
                    comment: "Map `{ENV_KEY: secret://<hex>}` produced by `request_secret`.",
                    required: true,
                },
            ],
            outputs: vec![
                FieldSchema {
                    name: "ok",
                    ty: TypeSchema::Bool,
                    comment: "True if initialize + tools/list succeeded.",
                    required: true,
                },
                FieldSchema {
                    name: "tools",
                    ty: TypeSchema::Option(Box::new(TypeSchema::Array(Box::new(TypeSchema::Ref(
                        "McpRemoteTool",
                    ))))),
                    comment: "Tools advertised by the candidate. Present iff `ok`.",
                    required: false,
                },
                FieldSchema {
                    name: "error",
                    ty: TypeSchema::Option(Box::new(TypeSchema::String)),
                    comment: "Error string. Present iff `ok` is false.",
                    required: false,
                },
            ],
        },
        "install_and_connect" => ControllerSchema {
            namespace: "mcp_setup",
            function: "install_and_connect",
            description: "Commit: persist the install + secrets (consuming the refs), then connect immediately and return the tool list.",
            inputs: vec![
                FieldSchema {
                    name: "qualified_name",
                    ty: TypeSchema::String,
                    comment: "Registry qualified name.",
                    required: true,
                },
                FieldSchema {
                    name: "env_refs",
                    ty: TypeSchema::Map(Box::new(TypeSchema::String)),
                    comment: "Map `{ENV_KEY: secret://<hex>}`. Refs are consumed (removed from the in-memory map) on success.",
                    required: true,
                },
            ],
            outputs: vec![
                FieldSchema {
                    name: "server_id",
                    ty: TypeSchema::String,
                    comment: "Freshly-minted server UUID.",
                    required: true,
                },
                FieldSchema {
                    name: "qualified_name",
                    ty: TypeSchema::String,
                    comment: "Registry qualified name the install was made from — echoed back so a caller need not correlate on the request.",
                    required: true,
                },
                FieldSchema {
                    name: "status",
                    ty: TypeSchema::String,
                    comment: "`connected` or `installed_disconnected` (install succeeded, connect failed).",
                    required: true,
                },
                FieldSchema {
                    name: "tools",
                    ty: TypeSchema::Option(Box::new(TypeSchema::Array(Box::new(TypeSchema::Ref(
                        "McpTool",
                    ))))),
                    comment: "Tool list iff `status == connected`.",
                    required: false,
                },
                FieldSchema {
                    name: "error",
                    ty: TypeSchema::Option(Box::new(TypeSchema::String)),
                    comment: "Connect error iff `status != connected`.",
                    required: false,
                },
            ],
        },
        _ => ControllerSchema {
            namespace: "mcp_setup",
            function: "unknown",
            description: "Unknown mcp_setup controller function.",
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
