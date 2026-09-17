//! OpenHuman policy admission for a `spawn_parallel_agents` task batch:
//! identity, the parent's subagent allowlist, the integrations toolkit
//! requirement, and whether a worker can write the shared workspace at all —
//! plus the crate-side arbitration over the resulting workspace claims.
//!
//! **Write safety.** Whether a worker *needs* a claim on the shared workspace is
//! an OpenHuman decision — it reads sandbox mode, tool permissions and the
//! isolation request. Whether the claims of a whole batch can be granted
//! together is not, and goes through
//! [`plan_shared_workspace_dispatch`](tinyagents_graph::parallel::plan_shared_workspace_dispatch).
//! The rejection sentences stay here, which is why the crate reports conflicts
//! as data.

use std::collections::HashMap;
use std::path::PathBuf;

use tinyagents_graph::parallel::{
    parse_relative_claim_paths, plan_shared_workspace_dispatch, ClaimConflict, ClaimPathError,
    DispatchMode, WorkspaceClaim,
};

use crate::agent::harness::definition::{
    AgentDefinition, AgentDefinitionRegistry, SandboxMode, ToolScope,
};
use crate::agent::harness::fork_context::ParentExecutionContext;
use crate::agent::orchestration::worktree::BaseRef;
use crate::tools::PermissionLevel;

use super::request::ParallelAgentTask;

/// Prepared worker ready for the live dispatch/worker phases.
pub(crate) struct PreparedParallelTask {
    pub(crate) definition: AgentDefinition,
    pub(crate) prompt: String,
    pub(crate) task: ParallelAgentTask,
    pub(crate) task_id: String,
    pub(crate) dispatch_mode: WorkerDispatchMode,
}

impl PreparedParallelTask {
    /// How this worker will be dispatched relative to its siblings.
    ///
    /// Exposed so the write-safety decision can be asserted directly: it is the
    /// one property of a preflight whose regression corrupts a shared checkout
    /// silently rather than failing a run.
    pub(crate) fn dispatch_mode(&self) -> WorkerDispatchMode {
        self.dispatch_mode
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ParallelTaskRejectionKind {
    MissingAgentOrPrompt,
    UnknownAgent,
    OutsideAllowlist,
    MissingToolkit,
    RequiresIsolation,
}

pub(crate) struct ParallelTaskRejection {
    pub(crate) task_id: String,
    pub(crate) agent_id: String,
    pub(crate) error: String,
    pub(crate) ownership: Option<String>,
    pub(crate) kind: ParallelTaskRejectionKind,
}

pub(crate) enum SpawnParallelTaskPreflight {
    Prepared(PreparedParallelTask),
    Rejected(ParallelTaskRejection),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WorkerDispatchMode {
    Parallel,
    SerialSharedWorkspaceWrite,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ParallelWorktreeRequest {
    SharedWorkspace,
    Isolated { base_ref: BaseRef },
}

pub(crate) fn worktree_request_for_task(task: &ParallelAgentTask) -> ParallelWorktreeRequest {
    let isolated = task
        .isolation
        .as_deref()
        .map(str::trim)
        .map(|s| s.eq_ignore_ascii_case("worktree"))
        .unwrap_or(false);
    if isolated {
        ParallelWorktreeRequest::Isolated {
            base_ref: BaseRef::parse(task.base_ref.as_deref()),
        }
    } else {
        ParallelWorktreeRequest::SharedWorkspace
    }
}

fn disallowed_tool_matches(disallowed: &[String], name: &str) -> bool {
    disallowed.iter().any(|entry| {
        if let Some(prefix) = entry.strip_suffix('*') {
            name.starts_with(prefix)
        } else {
            entry == name
        }
    })
}

fn definition_visible_tool_permissions(
    definition: &AgentDefinition,
    parent: &ParentExecutionContext,
) -> Vec<(String, PermissionLevel)> {
    let skill_prefix = definition
        .skill_filter
        .as_ref()
        .map(|skill| format!("{skill}__"));
    parent
        .all_tools
        .iter()
        .filter_map(|tool| {
            let name = tool.name();
            if disallowed_tool_matches(&definition.disallowed_tools, name) {
                return None;
            }
            if let Some(prefix) = skill_prefix.as_deref() {
                if !name.starts_with(prefix) {
                    return None;
                }
            }
            let allowed = match &definition.tools {
                ToolScope::Wildcard => true,
                ToolScope::Named(names) => {
                    names.iter().any(|allowed| allowed == name)
                        || definition.extra_tools.iter().any(|extra| extra == name)
                        || (crate::inference::tokenjuice::is_recovery_tool(name)
                            && !names.is_empty())
                }
            };
            allowed.then(|| (name.to_string(), tool.permission_level()))
        })
        .collect()
}

fn shared_workspace_write_capable_tools(
    definition: &AgentDefinition,
    parent: &ParentExecutionContext,
) -> Vec<String> {
    let mut write_capable_tools = definition_visible_tool_permissions(definition, parent)
        .into_iter()
        .filter(|(_, level)| *level > PermissionLevel::ReadOnly)
        .map(|(name, level)| format!("{name}:{level}"))
        .collect::<Vec<_>>();
    write_capable_tools.sort();
    write_capable_tools.dedup();
    write_capable_tools
}

fn shared_workspace_write_preview(write_capable_tools: &[String]) -> String {
    let preview = write_capable_tools
        .iter()
        .take(6)
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");
    let suffix = if write_capable_tools.len() > 6 {
        format!(", +{} more", write_capable_tools.len() - 6)
    } else {
        String::new()
    };
    format!("{preview}{suffix}")
}

/// Parse OpenHuman's `files: a.rs, b.rs` ownership syntax into claimed paths.
///
/// The `files:` prefix is this tool's parameter shape, so it is stripped here;
/// validating what follows is generic and belongs to
/// [`parse_relative_claim_paths`]. Its typed rejection is rendered into the
/// sentence the model reads at this boundary, which is why the crate returns
/// data rather than a message.
fn ownership_file_paths(ownership: Option<&str>) -> Result<Vec<PathBuf>, String> {
    let Some(ownership) = ownership.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(Vec::new());
    };
    let Some(rest) = ownership.strip_prefix("files:") else {
        return Ok(Vec::new());
    };
    parse_relative_claim_paths(rest).map_err(|err| {
        let raw = match &err {
            ClaimPathError::Absolute { raw } | ClaimPathError::Escaping { raw } => raw,
        };
        format!("ownership path '{raw}' must be a relative file path under the workspace")
    })
}

fn shared_workspace_write_claim(
    task: &ParallelAgentTask,
    definition: &AgentDefinition,
    parent: &ParentExecutionContext,
) -> Result<Option<Vec<PathBuf>>, String> {
    if matches!(
        worktree_request_for_task(task),
        ParallelWorktreeRequest::Isolated { .. }
    ) {
        return Ok(None);
    }
    if matches!(definition.sandbox_mode, SandboxMode::ReadOnly) {
        return Ok(None);
    }
    let write_capable_tools = shared_workspace_write_capable_tools(definition, parent);
    if write_capable_tools.is_empty() {
        return Ok(None);
    }
    let paths = ownership_file_paths(task.ownership.as_deref())?;
    if paths.is_empty() {
        return Err(format!(
            "agent '{}' can use write/execute tools in the shared workspace ({}); \
             set isolation=\"worktree\" for edit-capable parallel workers, use a read-only agent, \
             or provide disjoint files: ownership for serial fallback",
            definition.id,
            shared_workspace_write_preview(&write_capable_tools)
        ));
    }
    Ok(Some(paths))
}

pub(crate) fn snapshot_agent_definitions(
    registry: &AgentDefinitionRegistry,
) -> HashMap<String, AgentDefinition> {
    registry
        .list()
        .into_iter()
        .map(|definition| (definition.id.clone(), definition.clone()))
        .collect()
}

/// One task that cleared every OpenHuman policy gate and is awaiting the
/// shared-workspace arbitration verdict.
struct AdmittedParallelTask {
    definition: AgentDefinition,
    prompt: String,
    task: ParallelAgentTask,
    task_id: String,
    /// What this worker needs from the shared workspace, in the crate's terms.
    claim: WorkspaceClaim,
}

pub(crate) fn prepare_spawn_parallel_tasks_from_defs(
    tasks: Vec<ParallelAgentTask>,
    definitions: &HashMap<String, AgentDefinition>,
    parent: &ParentExecutionContext,
) -> Vec<SpawnParallelTaskPreflight> {
    // Pass 1 — OpenHuman policy. Identity, the parent's subagent allowlist, the
    // integrations toolkit requirement, and whether a worker can write the
    // shared workspace at all are all product decisions, so they are settled
    // here and rejected in their own vocabulary. What survives carries a
    // `WorkspaceClaim` describing only what the arbiter needs to know.
    enum Admission {
        Admitted(Box<AdmittedParallelTask>),
        Rejected(ParallelTaskRejection),
    }

    let admissions: Vec<Admission> = tasks
        .into_iter()
        .map(|task| {
            let agent_id = task.agent_id.trim().to_string();
            let prompt = task.prompt.trim().to_string();
            let task_id = format!("sub-{}", uuid::Uuid::new_v4());

            if agent_id.is_empty() || prompt.is_empty() {
                return Admission::Rejected(ParallelTaskRejection {
                    task_id,
                    agent_id,
                    error: "agent_id and prompt are required".to_string(),
                    ownership: task.ownership,
                    kind: ParallelTaskRejectionKind::MissingAgentOrPrompt,
                });
            }

            let Some(definition) = definitions.get(&agent_id).cloned() else {
                return Admission::Rejected(ParallelTaskRejection {
                    task_id,
                    agent_id: agent_id.clone(),
                    error: format!("unknown agent_id '{agent_id}'"),
                    ownership: task.ownership,
                    kind: ParallelTaskRejectionKind::UnknownAgent,
                });
            };

            if !parent.allowed_subagent_ids.contains(&definition.id) {
                return Admission::Rejected(ParallelTaskRejection {
                    task_id,
                    agent_id: definition.id.clone(),
                    error: format!(
                        "agent '{}' is not in parent agent '{}' subagents.allowlist",
                        definition.id, parent.agent_definition_id
                    ),
                    ownership: task.ownership,
                    kind: ParallelTaskRejectionKind::OutsideAllowlist,
                });
            }

            if definition.id == "integrations_agent"
                && task
                    .toolkit
                    .as_ref()
                    .map(|s| s.trim().is_empty())
                    .unwrap_or(true)
            {
                return Admission::Rejected(ParallelTaskRejection {
                    task_id,
                    agent_id,
                    error: "integrations_agent requires toolkit".to_string(),
                    ownership: task.ownership,
                    kind: ParallelTaskRejectionKind::MissingToolkit,
                });
            }

            let claim = match shared_workspace_write_claim(&task, &definition, parent) {
                // Needs the shared workspace and declares what it owns.
                Ok(Some(paths)) => WorkspaceClaim::writing(task_id.clone(), paths),
                // Cannot collide. Both arms plan as parallel, but they say so
                // for different reasons and the claim should carry the real one:
                // an isolated worker has its own root, a shared one simply never
                // writes.
                Ok(None) => {
                    if matches!(
                        worktree_request_for_task(&task),
                        ParallelWorktreeRequest::Isolated { .. }
                    ) {
                        WorkspaceClaim::isolated(task_id.clone())
                    } else {
                        WorkspaceClaim::read_only(task_id.clone())
                    }
                }
                Err(error) => {
                    return Admission::Rejected(ParallelTaskRejection {
                        task_id,
                        agent_id: definition.id.clone(),
                        error,
                        ownership: task.ownership,
                        kind: ParallelTaskRejectionKind::RequiresIsolation,
                    });
                }
            };

            Admission::Admitted(Box::new(AdmittedParallelTask {
                definition,
                prompt,
                task,
                task_id,
                claim,
            }))
        })
        .collect();

    // Pass 2 — arbitration. One planner call over every admitted claim, in input
    // order, so the verdict is a pure function of the request rather than of
    // which worker happened to be considered first. `shared_workspace_write_claim`
    // has already ruled out the unbounded-write case, so the only conflict the
    // planner can report here is an overlap.
    let claims: Vec<WorkspaceClaim> = admissions
        .iter()
        .filter_map(|admission| match admission {
            Admission::Admitted(admitted) => Some(admitted.claim.clone()),
            Admission::Rejected(_) => None,
        })
        .collect();
    let plan = plan_shared_workspace_dispatch(&claims);
    let conflicts: HashMap<usize, &ClaimConflict> = plan
        .conflicts
        .iter()
        .map(|(index, conflict)| (*index, conflict))
        .collect();

    let mut admitted_index = 0usize;
    admissions
        .into_iter()
        .map(|admission| {
            let admitted = match admission {
                Admission::Rejected(rejection) => {
                    return SpawnParallelTaskPreflight::Rejected(rejection);
                }
                Admission::Admitted(admitted) => admitted,
            };
            let index = admitted_index;
            admitted_index += 1;

            let AdmittedParallelTask {
                definition,
                prompt,
                task,
                task_id,
                claim: _,
            } = *admitted;

            if let Some(conflict) = conflicts.get(&index) {
                return SpawnParallelTaskPreflight::Rejected(ParallelTaskRejection {
                    task_id,
                    agent_id: definition.id.clone(),
                    error: shared_workspace_conflict_message(&definition.id, conflict),
                    ownership: task.ownership,
                    kind: ParallelTaskRejectionKind::RequiresIsolation,
                });
            }

            let dispatch_mode = match plan.modes.get(index).copied().flatten() {
                Some(DispatchMode::Serial) => WorkerDispatchMode::SerialSharedWorkspaceWrite,
                // A claim the planner neither serialized nor rejected cannot
                // collide, so it is safe to fan out.
                Some(DispatchMode::Parallel) | None => WorkerDispatchMode::Parallel,
            };

            let prompt = with_ownership_boundary(&prompt, task.ownership.as_deref());
            SpawnParallelTaskPreflight::Prepared(PreparedParallelTask {
                definition,
                prompt,
                task,
                task_id,
                dispatch_mode,
            })
        })
        .collect()
}

/// Render a claim conflict as the sentence the calling model reads.
///
/// The crate reports conflicts as data precisely so this phrasing — the
/// `isolation="worktree"` remedy, the `files:` vocabulary — stays a product
/// decision rather than becoming API.
fn shared_workspace_conflict_message(agent_id: &str, conflict: &ClaimConflict) -> String {
    match conflict {
        ClaimConflict::Overlap {
            other_worker_id,
            path,
            ..
        } => format!(
            "agent '{agent_id}' requested shared-workspace write access to '{}' but it overlaps with serial worker {other_worker_id}; set isolation=\"worktree\" or use disjoint files: ownership",
            path.display()
        ),
        ClaimConflict::UnboundedWrite { .. } => format!(
            "agent '{agent_id}' can write the shared workspace without declaring which files it owns; set isolation=\"worktree\" or provide disjoint files: ownership"
        ),
    }
}

pub(crate) fn with_ownership_boundary(prompt: &str, ownership: Option<&str>) -> String {
    match ownership.map(str::trim).filter(|s| !s.is_empty()) {
        Some(boundary) => format!(
            "[Ownership Boundary]\n{boundary}\n\n[Task]\n{prompt}\n\nDo not work outside the ownership boundary unless the parent explicitly asks you to."
        ),
        None => prompt.to_string(),
    }
}
