//! Schemas for unsaved flow drafts: the copilot's scratch space before a
//! graph is promoted into a saved flow.

use super::{
    draft_output, flow_output, id_input, require_approval_input, ControllerSchema, FieldSchema,
    TypeSchema,
};

/// Builds the schema for `function` when this group owns it.
pub(super) fn lookup(function: &str) -> Option<ControllerSchema> {
    match function {
        "draft_create" => Some(ControllerSchema {
            namespace: "flows",
            function: "draft_create",
            description: "Create a core-managed draft (a durable, non-live working copy of a graph) \
                          shared by the agent tools and the canvas. Never persists a flow.",
            inputs: vec![
                FieldSchema {
                    name: "name",
                    ty: TypeSchema::String,
                    comment: "Human-readable draft name (carried into the flow on promote).",
                    required: true,
                },
                FieldSchema {
                    name: "graph",
                    ty: TypeSchema::Json,
                    comment: "The (possibly incomplete) WorkflowGraph JSON to hold in the draft.",
                    required: true,
                },
                FieldSchema {
                    name: "flow_id",
                    ty: TypeSchema::Option(Box::new(TypeSchema::String)),
                    comment: "The saved flow this draft edits, if any (promote → update vs create).",
                    required: false,
                },
                FieldSchema {
                    name: "origin",
                    ty: TypeSchema::Option(Box::new(TypeSchema::String)),
                    comment: "Where the draft came from: `chat` | `canvas` | `import`. Defaults to `canvas`.",
                    required: false,
                },
            ],
            outputs: vec![draft_output()],
        }),
        "draft_get" => Some(ControllerSchema {
            namespace: "flows",
            function: "draft_get",
            description: "Fetch a draft by id.",
            inputs: vec![id_input("Identifier of the draft to fetch.")],
            outputs: vec![draft_output()],
        }),
        "draft_update" => Some(ControllerSchema {
            namespace: "flows",
            function: "draft_update",
            description: "Patch a draft's name/graph/flow_id (any provided field) and bump its \
                          updated_at. Never persists a flow.",
            inputs: vec![
                id_input("Identifier of the draft to update."),
                FieldSchema {
                    name: "name",
                    ty: TypeSchema::Option(Box::new(TypeSchema::String)),
                    comment: "New name, if changing it.",
                    required: false,
                },
                FieldSchema {
                    name: "graph",
                    ty: TypeSchema::Option(Box::new(TypeSchema::Json)),
                    comment: "New graph JSON, if changing it.",
                    required: false,
                },
                FieldSchema {
                    name: "flow_id",
                    ty: TypeSchema::Option(Box::new(TypeSchema::String)),
                    comment: "New linked flow id, if changing it.",
                    required: false,
                },
            ],
            outputs: vec![draft_output()],
        }),
        "draft_list" => Some(ControllerSchema {
            namespace: "flows",
            function: "draft_list",
            description: "List all drafts, newest-updated first.",
            inputs: vec![],
            outputs: vec![FieldSchema {
                name: "drafts",
                ty: TypeSchema::Array(Box::new(TypeSchema::Json)),
                comment: "The drafts (each { id, flow_id?, name, graph, origin, created_at, updated_at }).",
                required: true,
            }],
        }),
        "draft_delete" => Some(ControllerSchema {
            namespace: "flows",
            function: "draft_delete",
            description: "Delete a draft by id (idempotent).",
            inputs: vec![id_input("Identifier of the draft to delete.")],
            outputs: vec![FieldSchema {
                name: "result",
                ty: TypeSchema::Json,
                comment: "`{ id, deleted }` — `deleted` is false if the id was already absent.",
                required: true,
            }],
        }),
        "draft_promote" => Some(ControllerSchema {
            namespace: "flows",
            function: "draft_promote",
            description: "Promote a draft into a saved flow through the same create/update gates \
                          (structural validation, forced require_approval floor, born-disabled for \
                          automatic triggers), then delete the draft file. A draft with a flow_id \
                          updates that flow; otherwise it creates a new one.",
            inputs: vec![
                id_input("Identifier of the draft to promote."),
                require_approval_input(),
            ],
            outputs: vec![flow_output()],
        }),
        _ => None,
    }
}
