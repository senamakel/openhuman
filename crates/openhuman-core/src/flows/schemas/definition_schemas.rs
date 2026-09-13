//! Schemas for the saved-flow definition lifecycle: create, import,
//! validate, read, update, enable/disable, delete, revision history and the
//! approval/connection manifests derived from a graph.

use super::{
    expected_version_input, flow_connection_fields, flow_output, id_input, require_approval_input,
    strict_input, ControllerSchema, FieldSchema, TypeSchema,
};

/// Builds the schema for `function` when this group owns it.
pub(super) fn lookup(function: &str) -> Option<ControllerSchema> {
    match function {
        "create" => Some(ControllerSchema {
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
                strict_input(),
            ],
            outputs: vec![flow_output()],
        }),
        "duplicate" => Some(ControllerSchema {
            namespace: "flows",
            function: "duplicate",
            description: "Duplicate a saved flow: create an independent copy of its graph under a \
                          new id, with the name suffixed \" (copy)\". The copy is created DISABLED \
                          and is NOT schedule/trigger-bound, so it never immediately fires — the \
                          user enables it explicitly once reviewed. Run history does not carry over.",
            inputs: vec![id_input("Identifier of the flow to duplicate.")],
            outputs: vec![flow_output()],
        }),
        "validate" => Some(ControllerSchema {
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
        }),
        "import" => Some(ControllerSchema {
            namespace: "flows",
            function: "import",
            description: "Import a workflow definition WITHOUT saving it: parse a native tinyflows \
                          graph or an n8n workflow export, migrate + validate it, and return the \
                          normalized WorkflowGraph plus non-fatal import warnings. The caller opens \
                          the result on the canvas as a draft and Saves via the normal gate — \
                          import never persists or enables anything.",
            inputs: vec![
                FieldSchema {
                    name: "graph",
                    ty: TypeSchema::Json,
                    comment: "The workflow JSON to import: a tinyflows WorkflowGraph (native) or \
                              an n8n workflow export.",
                    required: true,
                },
                FieldSchema {
                    name: "format",
                    ty: TypeSchema::Option(Box::new(TypeSchema::Enum {
                        variants: vec!["native", "n8n", "auto"],
                    })),
                    comment: "Source format: `native` (tinyflows), `n8n`, or `auto` (default — \
                              detect by shape).",
                    required: false,
                },
            ],
            outputs: vec![
                FieldSchema {
                    name: "graph",
                    ty: TypeSchema::Json,
                    comment: "The normalized, migrated + validated WorkflowGraph, ready to open \
                              as an editable draft.",
                    required: true,
                },
                FieldSchema {
                    name: "warnings",
                    ty: TypeSchema::Array(Box::new(TypeSchema::String)),
                    comment: "Non-fatal import warnings (unmapped n8n node types, untranslated \
                              expressions, a synthesized/demoted trigger). Empty for a clean \
                              native import.",
                    required: true,
                },
            ],
        }),
        "get" => Some(ControllerSchema {
            namespace: "flows",
            function: "get",
            description: "Load one saved flow by id.",
            inputs: vec![id_input("Identifier of the flow to load.")],
            outputs: vec![flow_output()],
        }),
        "list" => Some(ControllerSchema {
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
        }),
        "list_connections" => Some(ControllerSchema {
            namespace: "flows",
            function: "list_connections",
            description: "List the connection sources a flow node's `connection_ref` can attach \
                          to: Composio connected accounts (kind `composio`) and stored HTTP \
                          credentials (kind `http`). Returns only non-secret metadata — ids, \
                          display labels, kind, and (for Composio) the connected account's own \
                          `platform_user_id` — never any secret material (OAuth/bearer tokens, \
                          passwords, and API keys stay server-side and are injected only at \
                          execution time).",
            inputs: vec![],
            outputs: vec![FieldSchema {
                name: "connections",
                ty: TypeSchema::Array(Box::new(TypeSchema::Object {
                    fields: flow_connection_fields(),
                })),
                comment: "Resolvable connections for the flows picker (composio + http), \
                          secret-free.",
                required: true,
            }],
        }),
        "update" => Some(ControllerSchema {
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
                strict_input(),
                expected_version_input(),
            ],
            outputs: vec![flow_output()],
        }),
        "delete" => Some(ControllerSchema {
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
        }),
        "set_enabled" => Some(ControllerSchema {
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
        }),
        "get_history" => Some(ControllerSchema {
            namespace: "flows",
            function: "get_history",
            description: "List a flow's revision history — prior graph snapshots captured on each \
                          update (capped, newest first). The safety rail behind rollback.",
            inputs: vec![
                id_input("Identifier of the flow whose history to list."),
                FieldSchema {
                    name: "limit",
                    ty: TypeSchema::Option(Box::new(TypeSchema::U64)),
                    comment: "Max revisions to return (defaults to the retention cap).",
                    required: false,
                },
            ],
            outputs: vec![FieldSchema {
                name: "revisions",
                ty: TypeSchema::Array(Box::new(TypeSchema::Json)),
                comment: "Revision snapshots: { id, flow_id, graph, name, require_approval, created_at }.",
                required: true,
            }],
        }),
        "rollback" => Some(ControllerSchema {
            namespace: "flows",
            function: "rollback",
            description: "Roll a flow back to a prior revision (restores that revision's graph \
                          through the normal update path — itself snapshotted, so rollback is \
                          undoable). Honours optimistic concurrency via expected_version.",
            inputs: vec![
                id_input("Identifier of the flow to roll back."),
                FieldSchema {
                    name: "revision_id",
                    ty: TypeSchema::String,
                    comment: "The revision (from get_history) to restore.",
                    required: true,
                },
                expected_version_input(),
            ],
            outputs: vec![flow_output()],
        }),
        "required_connections" => Some(ControllerSchema {
            namespace: "flows",
            function: "required_connections",
            description: "Compute which Composio toolkits a candidate graph needs and whether each \
                          is connected — the data behind the canvas/proposal \"Connect <toolkit>\" \
                          CTAs. Native oh: tools and http_request nodes need no connection.",
            inputs: vec![FieldSchema {
                name: "graph",
                ty: TypeSchema::Json,
                comment: "The WorkflowGraph to inspect.",
                required: true,
            }],
            outputs: vec![FieldSchema {
                name: "required_connections",
                ty: TypeSchema::Array(Box::new(TypeSchema::Json)),
                comment: "One per needed toolkit: { toolkit, status: connected|missing }.",
                required: true,
            }],
        }),
        "approval_manifest" => Some(ControllerSchema {
            namespace: "flows",
            function: "approval_manifest",
            description:
                "Compute the approval manifest for a saved flow (by id) or a candidate graph: \
                 every ApprovalGate permission a run will prompt for, joined against the flow's \
                 existing flow_tool_trust grants — the data behind the consolidated save+enable \
                 pre-authorization card. Entries carry kind approvable|blocked|dynamic|agent.",
            inputs: vec![
                FieldSchema {
                    name: "id",
                    ty: TypeSchema::Option(Box::new(TypeSchema::String)),
                    comment: "Saved flow id. Provide this or 'graph'.",
                    required: false,
                },
                FieldSchema {
                    name: "graph",
                    ty: TypeSchema::Option(Box::new(TypeSchema::Json)),
                    comment: "Candidate WorkflowGraph to inspect (no trust join without an id).",
                    required: false,
                },
            ],
            outputs: vec![
                FieldSchema {
                    name: "entries",
                    ty: TypeSchema::Array(Box::new(TypeSchema::Json)),
                    comment:
                        "One per relevant node/tool: {kind: approvable|blocked|dynamic|agent, \
                         node_id, tool_name?, label, class?}.",
                    required: true,
                },
                FieldSchema {
                    name: "missing",
                    ty: TypeSchema::Array(Box::new(TypeSchema::String)),
                    comment: "Approvable trust keys the flow does not yet hold.",
                    required: true,
                },
                FieldSchema {
                    name: "already_trusted",
                    ty: TypeSchema::Array(Box::new(TypeSchema::String)),
                    comment: "Approvable trust keys already granted to this flow. Empty when \
                              gate_installed is false — no gate means no grant was ever made.",
                    required: true,
                },
                FieldSchema {
                    name: "gate_installed",
                    ty: TypeSchema::Bool,
                    comment:
                        "False when the approval gate is disabled — nothing ever prompts, so \
                         missing is empty by definition.",
                    required: true,
                },
            ],
        }),
        _ => None,
    }
}
