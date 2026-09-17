//! Per-function [`ControllerSchema`] definitions for the `todos` namespace,
//! plus the small `FieldSchema` builder helpers they share.

use crate::core::{ControllerSchema, FieldSchema, TypeSchema};

pub(super) fn schemas(function: &str) -> ControllerSchema {
    match function {
        "list" => ControllerSchema {
            namespace: "todos",
            function: "list",
            description:
                "Return the current todo list for a conversation thread as cards + markdown.",
            inputs: vec![thread_id_input()],
            outputs: vec![snapshot_output()],
        },
        "add" => ControllerSchema {
            namespace: "todos",
            function: "add",
            description: "Append a new todo card to a conversation thread's list.",
            inputs: vec![
                thread_id_input(),
                required_string("content", "Card title / description."),
                optional_string("status", "Initial status (todo|in_progress|blocked|done)."),
                optional_string("objective", "Task objective / desired outcome."),
                string_array_input("plan", "Ordered lightweight execution steps."),
                optional_string("assignedAgent", "Agent id expected to pick up this task."),
                string_array_input(
                    "allowedTools",
                    "Task-local allowed tool names or toolkit slugs.",
                ),
                optional_string(
                    "approvalMode",
                    "Task approval mode: required | not_required.",
                ),
                string_array_input(
                    "acceptanceCriteria",
                    "Checklist required before the task is done.",
                ),
                string_array_input("evidence", "Verification output, links, files, or notes."),
                optional_string("notes", "Free-text notes."),
                optional_string("blocker", "Reason the card is blocked, if any."),
                FieldSchema {
                    name: "sourceMetadata",
                    ty: TypeSchema::Json,
                    comment: "Originating task-source identifiers ({provider, external_id, …}) \
                              stamped onto a card promoted from the task-sources inbox.",
                    required: false,
                },
            ],
            outputs: vec![snapshot_output()],
        },
        "edit" => ControllerSchema {
            namespace: "todos",
            function: "edit",
            description: "Edit an existing todo card by id. Any omitted field is left unchanged.",
            inputs: vec![
                thread_id_input(),
                required_string("id", "Card identifier returned by `add` / `list`."),
                optional_string("content", "New title / description."),
                optional_string("status", "New status."),
                optional_string("objective", "Task objective / desired outcome."),
                string_array_input("plan", "Ordered lightweight execution steps."),
                optional_string("assignedAgent", "Agent id expected to pick up this task."),
                string_array_input(
                    "allowedTools",
                    "Task-local allowed tool names or toolkit slugs.",
                ),
                optional_string(
                    "approvalMode",
                    "Task approval mode: required | not_required (pass null to clear).",
                ),
                string_array_input(
                    "acceptanceCriteria",
                    "Checklist required before the task is done.",
                ),
                string_array_input("evidence", "Verification output, links, files, or notes."),
                optional_string("notes", "New notes (pass empty string to clear)."),
                optional_string(
                    "blocker",
                    "New blocker reason (pass empty string to clear).",
                ),
            ],
            outputs: vec![snapshot_output()],
        },
        "update_status" => ControllerSchema {
            namespace: "todos",
            function: "update_status",
            description: "Update only the status of a todo card.",
            inputs: vec![
                thread_id_input(),
                required_string("id", "Card identifier."),
                required_string(
                    "status",
                    "New status (todo|awaiting_approval|ready|in_progress|blocked|done|rejected).",
                ),
            ],
            outputs: vec![snapshot_output()],
        },
        "set_session_thread" => ControllerSchema {
            namespace: "todos",
            function: "set_session_thread",
            description: "Link a card to its agent session's conversation thread so the UI can \
                          offer a \"View session\" jump. Empty/absent sessionThreadId clears the link.",
            inputs: vec![
                thread_id_input(),
                required_string("id", "Card identifier."),
                optional_string(
                    "sessionThreadId",
                    "Conversation thread id of the card's agent session; omit or empty to clear.",
                ),
            ],
            outputs: vec![snapshot_output()],
        },
        "decide_plan" => ControllerSchema {
            namespace: "todos",
            function: "decide_plan",
            description: "Approve or reject a card awaiting plan approval \
                          (approve → ready/runnable; reject → rejected).",
            inputs: vec![
                thread_id_input(),
                required_string("id", "Card identifier."),
                FieldSchema {
                    name: "approve",
                    ty: TypeSchema::Bool,
                    comment: "true to approve (card becomes runnable), false to reject.",
                    required: true,
                },
            ],
            outputs: vec![snapshot_output()],
        },
        "revise_plan" => ControllerSchema {
            namespace: "todos",
            function: "revise_plan",
            description: "Reject every card awaiting plan approval so the orchestrator can \
                          re-plan from the user's feedback (the feedback is sent back into the \
                          thread as a message by the caller). Idempotent no-op when nothing is \
                          awaiting.",
            inputs: vec![
                thread_id_input(),
                optional_string(
                    "feedback",
                    "User's free-text revision request (recorded for logging/audit).",
                ),
            ],
            outputs: vec![snapshot_output()],
        },
        "remove" => ControllerSchema {
            namespace: "todos",
            function: "remove",
            description: "Remove a todo card from a thread's list.",
            inputs: vec![thread_id_input(), required_string("id", "Card identifier.")],
            outputs: vec![snapshot_output()],
        },
        "replace" => ControllerSchema {
            namespace: "todos",
            function: "replace",
            description: "Wholesale-replace the todo list for a thread.",
            inputs: vec![thread_id_input(), replace_cards_input()],
            outputs: vec![snapshot_output()],
        },
        "clear" => ControllerSchema {
            namespace: "todos",
            function: "clear",
            description: "Empty the todo list for a thread.",
            inputs: vec![thread_id_input()],
            outputs: vec![snapshot_output()],
        },
        "run_list" => ControllerSchema {
            namespace: "todos",
            function: "run_list",
            description: "List durable run records for a thread, optionally filtered by card id.",
            inputs: vec![
                thread_id_input(),
                optional_string("cardId", "Filter runs by card identifier."),
            ],
            outputs: vec![FieldSchema {
                name: "runs",
                ty: TypeSchema::Json,
                comment: "Array of TaskRun objects.",
                required: true,
            }],
        },
        "run_get" => ControllerSchema {
            namespace: "todos",
            function: "run_get",
            description: "Get a single run record by its run id.",
            inputs: vec![
                thread_id_input(),
                required_string("runId", "Run identifier."),
            ],
            outputs: vec![FieldSchema {
                name: "run",
                ty: TypeSchema::Json,
                comment: "TaskRun object or null if not found.",
                required: true,
            }],
        },
        "reclaim_stale" => ControllerSchema {
            namespace: "todos",
            function: "reclaim_stale",
            description: "Scan for stale/wedged runs and reclaim their cards. \
                 Cards are moved back to todo (re-dispatchable) or blocked \
                 (max reclaim count exceeded).",
            inputs: vec![
                thread_id_input(),
                FieldSchema {
                    name: "heartbeatStaleSecs",
                    ty: TypeSchema::Option(Box::new(TypeSchema::U64)),
                    comment: "Heartbeat staleness threshold in seconds (default 300).",
                    required: false,
                },
                FieldSchema {
                    name: "claimTtlSecs",
                    ty: TypeSchema::Option(Box::new(TypeSchema::U64)),
                    comment: "Claim TTL in seconds (default 3600).",
                    required: false,
                },
                FieldSchema {
                    name: "maxReclaimCount",
                    ty: TypeSchema::Option(Box::new(TypeSchema::U64)),
                    comment: "Max reclaims before parking as blocked (default 3).",
                    required: false,
                },
            ],
            outputs: vec![FieldSchema {
                name: "result",
                ty: TypeSchema::Json,
                comment: "Object with reclaimedCount, blockedCount, and details array.",
                required: true,
            }],
        },
        _ => ControllerSchema {
            namespace: "todos",
            function: "unknown",
            description: "Unknown todos controller function.",
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

fn thread_id_input() -> FieldSchema {
    FieldSchema {
        name: "thread_id",
        ty: TypeSchema::String,
        comment: "Conversation thread identifier (same id used by `threads.task_board_*`).",
        required: true,
    }
}

fn required_string(name: &'static str, comment: &'static str) -> FieldSchema {
    FieldSchema {
        name,
        ty: TypeSchema::String,
        comment,
        required: true,
    }
}

fn optional_string(name: &'static str, comment: &'static str) -> FieldSchema {
    FieldSchema {
        name,
        ty: TypeSchema::Option(Box::new(TypeSchema::String)),
        comment,
        required: false,
    }
}

fn string_array_input(name: &'static str, comment: &'static str) -> FieldSchema {
    FieldSchema {
        name,
        ty: TypeSchema::Option(Box::new(TypeSchema::Array(Box::new(TypeSchema::String)))),
        comment,
        required: false,
    }
}

/// The `cards` input of `todos.replace`, spelled out field by field.
///
/// This was a bare `TypeSchema::Json` commented "Array of card objects (id may
/// be empty — server generates)", which is not enough to construct one and was
/// actively misleading in two ways (#6087):
///
///  * `handle_replace` deserializes each entry into `TaskBoardCard`, whose
///    `id`, `title` and `status` carry **no** `#[serde(default)]` — so all
///    three keys are mandatory. "id may be empty" is true of the *string* and
///    false of the *key*: omitting it fails with ``missing field `id` ``.
///  * the text field is `title`, while the sibling `todos.add` / `todos.edit`
///    inputs in this same namespace call it `content`. A caller who reached for
///    the namespace's own vocabulary got ``missing field `title` ``.
///
/// Names below are the wire names: `TaskBoardCard` is
/// `#[serde(rename_all = "camelCase")]` and `TaskCardStatus` is
/// `#[serde(rename_all = "snake_case")]`, so the declaration must use
/// `assignedAgent` (not `assigned_agent`) and `in_progress` (not `inProgress`).
/// Every optional field carries `#[serde(default)]` upstream, so the optional
/// markings here are load-bearing rather than decorative.
fn replace_cards_input() -> FieldSchema {
    FieldSchema {
        name: "cards",
        ty: TypeSchema::Array(Box::new(TypeSchema::Object {
            fields: vec![
                FieldSchema {
                    name: "id",
                    ty: TypeSchema::String,
                    comment: "Stable card id (`task-<n>`). The KEY is required; \
                              pass an empty string to have the server generate one.",
                    required: true,
                },
                FieldSchema {
                    name: "title",
                    ty: TypeSchema::String,
                    comment: "One-line title. NOTE: `todos.add` / `todos.edit` \
                              call this same field `content`; here it is `title`.",
                    required: true,
                },
                FieldSchema {
                    name: "status",
                    ty: TypeSchema::Enum {
                        variants: vec![
                            "todo",
                            "awaiting_approval",
                            "ready",
                            "in_progress",
                            "blocked",
                            "done",
                            "rejected",
                        ],
                    },
                    comment: "Lifecycle state. At most one card may be `in_progress`.",
                    required: true,
                },
                optional_string("objective", "Richer objective for the card."),
                defaulted_field(
                    "plan",
                    TypeSchema::Array(Box::new(TypeSchema::String)),
                    "Ordered plan steps. Omit the key to default to []; `null` is rejected.",
                ),
                optional_string("assignedAgent", "Agent assigned to run this card."),
                defaulted_field(
                    "allowedTools",
                    TypeSchema::Array(Box::new(TypeSchema::String)),
                    "Tools the assigned agent may use. Omit to default to []; `null` is rejected.",
                ),
                FieldSchema {
                    name: "approvalMode",
                    // `TaskApprovalMode` is a two-variant enum, not a free
                    // string: declaring `Option(String)` advertised every
                    // string as valid, so a catalog-conforming `"sometimes"`
                    // would come back `invalid params`. Wrapped in `Option`
                    // because the field really is `Option<TaskApprovalMode>`
                    // upstream, so `null` genuinely is accepted here — unlike
                    // the `defaulted_field` group above.
                    ty: TypeSchema::Option(Box::new(TypeSchema::Enum {
                        variants: vec!["required", "not_required"],
                    })),
                    comment: "Plan-approval mode, when the card is gated. \
                              `null` clears it.",
                    required: false,
                },
                defaulted_field(
                    "acceptanceCriteria",
                    TypeSchema::Array(Box::new(TypeSchema::String)),
                    "Acceptance criteria that define \"done\". Omit to default to []; `null` is rejected.",
                ),
                defaulted_field(
                    "evidence",
                    TypeSchema::Array(Box::new(TypeSchema::String)),
                    "Evidence gathered toward completion. Omit to default to []; `null` is rejected.",
                ),
                optional_string("notes", "Free-form notes."),
                optional_string("blocker", "Reason, when `status == blocked`."),
                optional_string(
                    "sessionThreadId",
                    "Thread the card's own agent session runs in.",
                ),
                FieldSchema {
                    name: "sourceMetadata",
                    ty: TypeSchema::Option(Box::new(TypeSchema::Json)),
                    comment: "Provenance blob carried through untouched.",
                    required: false,
                },
                defaulted_field(
                    "order",
                    // `TaskBoardCard::order` is a `u32`, but `TypeSchema` has
                    // no narrower unsigned type and no bounds, so `U64` is the
                    // closest available declaration and the ceiling has to be
                    // stated in prose. A value above `u32::MAX` passes schema
                    // validation and is then refused by the handler's
                    // deserialization. That imprecision is shared by every
                    // `TypeSchema::U64` declaration in the catalog (200-odd of
                    // them), so closing it means adding a bounded integer to
                    // `core::TypeSchema` rather than editing this one field —
                    // tracked as #6137. Drop this caveat when that lands.
                    TypeSchema::U64,
                    "Sort position, 0..=4294967295 (u32). Omit to default to 0; \
                     `null` is rejected, and a value above the u32 ceiling is \
                     refused by the handler rather than by schema validation.",
                ),
                defaulted_field(
                    "updatedAt",
                    TypeSchema::String,
                    "Last-update stamp, server-maintained. Omit it; `null` is rejected.",
                ),
            ],
        })),
        comment: "Full replacement list. Each entry MUST carry `id`, `title` and \
                  `status`; every other field is optional. Note `title`, not \
                  `content` — see the field comments.",
        required: true,
    }
}

/// A `todos.replace` card field that upstream `#[serde(default)]`s.
///
/// Declared with its **non-`Option`** type and `required: false`, which is the
/// accurate statement of the contract: the key may be *omitted* (serde supplies
/// the default) but may not be sent as `null`. `TaskBoardCard`'s `plan`,
/// `allowedTools`, `acceptanceCriteria` and `evidence` are `Vec<String>`,
/// `order` is `u32` and `updatedAt` is `String` — none is an `Option`, so
/// `null` fails deserialization with `invalid type: null`.
///
/// Declaring these as `Option(...)` would advertise `null` as valid and put the
/// catalog right back to describing a call the handler rejects, which is the
/// defect #6087 exists to remove.
fn defaulted_field(name: &'static str, ty: TypeSchema, comment: &'static str) -> FieldSchema {
    FieldSchema {
        name,
        ty,
        comment,
        required: false,
    }
}

fn snapshot_output() -> FieldSchema {
    FieldSchema {
        name: "snapshot",
        ty: TypeSchema::Json,
        comment: "Object with `threadId`, `cards`, and a `markdown` rendering of the list.",
        required: true,
    }
}
