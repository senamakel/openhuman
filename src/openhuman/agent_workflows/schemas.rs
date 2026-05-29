//! JSON-RPC / CLI controller surface for the agent_workflows domain.
//!
//! Exposes (namespace `workflows`, methods `openhuman.workflows_*`):
//! * `workflows.list` — enumerate WORKFLOW.md workflows discovered in the user
//!   home and (trusted) workspace.
//! * `workflows.read` — read one workflow's full definition (all phases) by id.
//! * `workflows.create` — scaffold a new WORKFLOW.md under the user/workspace
//!   scope.
//! * `workflows.uninstall` — remove a user-scope workflow.
//! * `workflows.phase` — resolve a phase's guidance + effective tool scope for a
//!   workflow id (the explicit / introspection counterpart to the harness's
//!   auto-injection).
//!
//! All controllers resolve the active workspace via the persisted config layer
//! so CLI and UI see the same catalog.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::core::all::{ControllerFuture, RegisteredController};
use crate::core::{ControllerSchema, FieldSchema, TypeSchema};
use crate::openhuman::config::Config;
use crate::rpc::RpcOutcome;

use super::discover::discover_workflows;
use super::ops::{
    create_workflow, read_workflow, uninstall_workflow, CreateWorkflowParams,
    UninstallWorkflowParams,
};
use super::select::{effective_tool_scope, phase_guidance};
use super::types::{ToolScope, Workflow, WorkflowPhase, WorkflowScope};

// ── Wire types ──────────────────────────────────────────────────────────

/// Catalog row for `workflows.list` — keeps the payload light (phase *names*
/// only, no bodies).
#[derive(Debug, Serialize)]
struct WorkflowSummary {
    id: String,
    name: String,
    description: String,
    when_to_use: Option<String>,
    tags: Vec<String>,
    phases: Vec<String>,
    scope: WorkflowScope,
    location: Option<String>,
    warnings: Vec<String>,
}

impl From<Workflow> for WorkflowSummary {
    fn from(w: Workflow) -> Self {
        let id = w.id().to_string();
        WorkflowSummary {
            id,
            name: w.name,
            description: w.description,
            when_to_use: w.when_to_use,
            tags: w.tags,
            phases: w.phases.keys().cloned().collect(),
            scope: w.scope,
            location: w.location.as_ref().map(|p| p.display().to_string()),
            warnings: w.warnings,
        }
    }
}

/// Full workflow definition for `workflows.read`.
#[derive(Debug, Serialize)]
struct WorkflowDetail {
    id: String,
    name: String,
    description: String,
    when_to_use: Option<String>,
    tags: Vec<String>,
    tools: Option<ToolScope>,
    phases: BTreeMap<String, WorkflowPhase>,
    scope: WorkflowScope,
    location: Option<String>,
    warnings: Vec<String>,
}

impl From<Workflow> for WorkflowDetail {
    fn from(w: Workflow) -> Self {
        let id = w.id().to_string();
        WorkflowDetail {
            id,
            name: w.name,
            description: w.description,
            when_to_use: w.when_to_use,
            tags: w.tags,
            tools: w.tools,
            phases: w.phases,
            scope: w.scope,
            location: w.location.as_ref().map(|p| p.display().to_string()),
            warnings: w.warnings,
        }
    }
}

#[derive(Debug, Serialize)]
struct WorkflowsListResult {
    workflows: Vec<WorkflowSummary>,
}

#[derive(Debug, Serialize)]
struct WorkflowsCreateResult {
    workflow: WorkflowDetail,
}

#[derive(Debug, Serialize)]
struct WorkflowsUninstallResult {
    name: String,
    removed_path: String,
    scope: WorkflowScope,
}

#[derive(Debug, Serialize)]
struct WorkflowsPhaseResult {
    workflow_id: String,
    phase: String,
    /// `true` when the workflow actually declares this phase.
    declared: bool,
    /// Rendered guidance block (rules + scripts listing), if any.
    guidance: Option<String>,
    /// Effective tool scope for this phase (phase override ∪ workflow default).
    tool_scope: Option<ToolScope>,
    /// Working-dir context providers requested by this phase.
    context: Vec<String>,
    /// Scripts that would run at this phase.
    scripts: Vec<String>,
}

// ── Params ──────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Default)]
struct WorkflowsListParams {}

#[derive(Debug, Deserialize)]
struct WorkflowIdParams {
    workflow_id: String,
}

#[derive(Debug, Deserialize)]
struct WorkflowsPhaseParams {
    workflow_id: String,
    phase: String,
}

// ── Registry ──────────────────────────────────────────────────────────────

pub fn all_controller_schemas() -> Vec<ControllerSchema> {
    vec![
        schemas("list"),
        schemas("read"),
        schemas("create"),
        schemas("uninstall"),
        schemas("phase"),
    ]
}

pub fn all_registered_controllers() -> Vec<RegisteredController> {
    vec![
        RegisteredController {
            schema: schemas("list"),
            handler: handle_list,
        },
        RegisteredController {
            schema: schemas("read"),
            handler: handle_read,
        },
        RegisteredController {
            schema: schemas("create"),
            handler: handle_create,
        },
        RegisteredController {
            schema: schemas("uninstall"),
            handler: handle_uninstall,
        },
        RegisteredController {
            schema: schemas("phase"),
            handler: handle_phase,
        },
    ]
}

pub fn schemas(function: &str) -> ControllerSchema {
    match function {
        "list" => ControllerSchema {
            namespace: "workflows",
            function: "list",
            description: "List WORKFLOW.md workflows discovered in the user home and trusted workspace.",
            inputs: vec![],
            outputs: vec![FieldSchema {
                name: "workflows",
                ty: TypeSchema::Array(Box::new(TypeSchema::Ref("WorkflowSummary"))),
                comment: "Discovered workflows (sorted by name; project-scope shadows user-scope).",
                required: true,
            }],
        },
        "read" => ControllerSchema {
            namespace: "workflows",
            function: "read",
            description: "Read one workflow's full definition (all phases) by id.",
            inputs: vec![workflow_id_input("Identifier (on-disk slug) of the workflow to read.")],
            outputs: vec![FieldSchema {
                name: "workflow",
                ty: TypeSchema::Ref("WorkflowDetail"),
                comment: "Full workflow definition including every phase.",
                required: true,
            }],
        },
        "create" => ControllerSchema {
            namespace: "workflows",
            function: "create",
            description: "Scaffold a new WORKFLOW.md workflow under the user or workspace scope.",
            inputs: vec![
                FieldSchema {
                    name: "name",
                    ty: TypeSchema::String,
                    comment: "Human-readable name (slugified into the on-disk directory).",
                    required: true,
                },
                FieldSchema {
                    name: "description",
                    ty: TypeSchema::String,
                    comment: "One-line description written into WORKFLOW.md frontmatter.",
                    required: true,
                },
                FieldSchema {
                    name: "when_to_use",
                    ty: TypeSchema::String,
                    comment: "Optional natural-language hint describing when this workflow applies.",
                    required: false,
                },
                FieldSchema {
                    name: "scope",
                    ty: TypeSchema::String,
                    comment: "Target scope: 'user' (default) or 'project' (requires trust marker).",
                    required: false,
                },
                FieldSchema {
                    name: "tags",
                    ty: TypeSchema::Array(Box::new(TypeSchema::String)),
                    comment: "Optional tags for the workflow.",
                    required: false,
                },
            ],
            outputs: vec![FieldSchema {
                name: "workflow",
                ty: TypeSchema::Ref("WorkflowDetail"),
                comment: "The newly created workflow, re-discovered through the standard pipeline.",
                required: true,
            }],
        },
        "uninstall" => ControllerSchema {
            namespace: "workflows",
            function: "uninstall",
            description: "Remove an installed user-scope workflow from ~/.openhuman/workflows/<name>/. Project-scope workflows are read-only. Rejects path separators and traversal; canonicalises before delete.",
            inputs: vec![FieldSchema {
                name: "name",
                ty: TypeSchema::String,
                comment: "Exact on-disk slug of the installed workflow (matches WorkflowSummary.id).",
                required: true,
            }],
            outputs: vec![FieldSchema {
                name: "result",
                ty: TypeSchema::Object {
                    fields: vec![
                        FieldSchema {
                            name: "name",
                            ty: TypeSchema::String,
                            comment: "Echo of the removed workflow slug.",
                            required: true,
                        },
                        FieldSchema {
                            name: "removed_path",
                            ty: TypeSchema::String,
                            comment: "Canonical on-disk path that was deleted.",
                            required: true,
                        },
                        FieldSchema {
                            name: "scope",
                            ty: TypeSchema::String,
                            comment: "Scope the uninstall applied to. Always 'user' today.",
                            required: true,
                        },
                    ],
                },
                comment: "Uninstall result payload.",
                required: true,
            }],
        },
        "phase" => ControllerSchema {
            namespace: "workflows",
            function: "phase",
            description: "Resolve a phase's guidance + effective tool scope + context providers for a workflow id.",
            inputs: vec![
                workflow_id_input("Identifier of the workflow whose phase to resolve."),
                FieldSchema {
                    name: "phase",
                    ty: TypeSchema::String,
                    comment: "Phase name, e.g. 'on_pick_up_task', 'on_close_task', 'on_enter_directory'.",
                    required: true,
                },
            ],
            outputs: vec![FieldSchema {
                name: "result",
                ty: TypeSchema::Ref("WorkflowsPhaseResult"),
                comment: "Resolved phase guidance, tool scope, context providers, and scripts.",
                required: true,
            }],
        },
        _other => ControllerSchema {
            namespace: "workflows",
            function: "unknown",
            description: "Unknown workflows controller function.",
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

fn workflow_id_input(comment: &'static str) -> FieldSchema {
    FieldSchema {
        name: "workflow_id",
        ty: TypeSchema::String,
        comment,
        required: true,
    }
}

// ── Handlers ──────────────────────────────────────────────────────────────

fn handle_list(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let _ = deserialize_params::<WorkflowsListParams>(params)?;
        tracing::debug!("[workflows][rpc] list");
        let workspace = resolve_workspace_dir().await;
        let trusted = super::discover::is_workspace_trusted(&workspace);
        let home = dirs::home_dir();
        let workflows = discover_workflows(home.as_deref(), Some(workspace.as_path()), trusted);
        tracing::debug!(
            count = workflows.len(),
            trusted,
            "[workflows][rpc] list result"
        );
        let summaries = workflows.into_iter().map(WorkflowSummary::from).collect();
        to_json(RpcOutcome::new(
            WorkflowsListResult {
                workflows: summaries,
            },
            Vec::new(),
        ))
    })
}

fn handle_read(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let p = deserialize_params::<WorkflowIdParams>(params)?;
        tracing::debug!(workflow_id = %p.workflow_id, "[workflows][rpc] read");
        let workspace = resolve_workspace_dir().await;
        match read_workflow(workspace.as_path(), p.workflow_id.trim()) {
            Ok(workflow) => to_json(RpcOutcome::new(
                WorkflowsCreateResult {
                    workflow: WorkflowDetail::from(workflow),
                },
                Vec::new(),
            )),
            Err(err) => {
                tracing::debug!(error = %err, "[workflows][rpc] read: rejected");
                Err(err)
            }
        }
    })
}

fn handle_create(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let p = deserialize_params::<CreateWorkflowParams>(params)?;
        tracing::debug!(name = %p.name, scope = ?p.scope, "[workflows][rpc] create");
        let workspace = resolve_workspace_dir().await;
        match create_workflow(workspace.as_path(), p) {
            Ok(workflow) => to_json(RpcOutcome::new(
                WorkflowsCreateResult {
                    workflow: WorkflowDetail::from(workflow),
                },
                Vec::new(),
            )),
            Err(err) => {
                tracing::debug!(error = %err, "[workflows][rpc] create: rejected");
                Err(err)
            }
        }
    })
}

fn handle_uninstall(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let p = deserialize_params::<UninstallWorkflowParams>(params)?;
        tracing::debug!(name = %p.name, "[workflows][rpc] uninstall");
        match uninstall_workflow(p, None) {
            Ok(outcome) => to_json(RpcOutcome::new(
                WorkflowsUninstallResult {
                    name: outcome.name,
                    removed_path: outcome.removed_path,
                    scope: outcome.scope,
                },
                Vec::new(),
            )),
            Err(err) => {
                tracing::debug!(error = %err, "[workflows][rpc] uninstall: rejected");
                Err(err)
            }
        }
    })
}

fn handle_phase(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let p = deserialize_params::<WorkflowsPhaseParams>(params)?;
        tracing::debug!(workflow_id = %p.workflow_id, phase = %p.phase, "[workflows][rpc] phase");
        let workspace = resolve_workspace_dir().await;
        let workflow = match read_workflow(workspace.as_path(), p.workflow_id.trim()) {
            Ok(w) => w,
            Err(err) => return Err(err),
        };
        let phase_name = p.phase.trim();
        let phase = workflow.phase(phase_name);
        let result = WorkflowsPhaseResult {
            workflow_id: workflow.id().to_string(),
            phase: phase_name.to_string(),
            declared: phase.is_some(),
            guidance: phase_guidance(&workflow, phase_name),
            tool_scope: effective_tool_scope(&workflow, phase_name),
            context: phase.map(|p| p.context.clone()).unwrap_or_default(),
            scripts: phase.map(|p| p.scripts.clone()).unwrap_or_default(),
        };
        to_json(RpcOutcome::new(result, Vec::new()))
    })
}

// ── Helpers ──────────────────────────────────────────────────────────────

async fn resolve_workspace_dir() -> PathBuf {
    match tokio::time::timeout(std::time::Duration::from_secs(30), Config::load_or_init()).await {
        Ok(Ok(cfg)) => cfg.workspace_dir,
        Ok(Err(err)) => {
            tracing::debug!(error = %err, "[workflows][rpc] config load failed; falling back to default workspace");
            fallback_workspace_dir()
        }
        Err(_) => {
            tracing::debug!(
                "[workflows][rpc] config load timed out; falling back to default workspace"
            );
            fallback_workspace_dir()
        }
    }
}

fn fallback_workspace_dir() -> PathBuf {
    crate::openhuman::config::default_root_openhuman_dir()
        .unwrap_or_else(|_| PathBuf::from(".openhuman"))
        .join("workspace")
}

fn deserialize_params<T: DeserializeOwned>(params: Map<String, Value>) -> Result<T, String> {
    serde_json::from_value(Value::Object(params)).map_err(|e| format!("invalid params: {e}"))
}

fn to_json<T: serde::Serialize>(outcome: RpcOutcome<T>) -> Result<Value, String> {
    outcome.into_cli_compatible_json()
}

#[cfg(test)]
#[path = "schemas_tests.rs"]
mod tests;
