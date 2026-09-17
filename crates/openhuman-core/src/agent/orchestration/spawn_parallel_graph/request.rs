//! Request parsing and structural validation for `spawn_parallel_agents`,
//! before any policy or workspace decision is made.

/// One requested worker in a `spawn_parallel_agents` call.
#[derive(Debug, Clone, serde::Deserialize)]
pub(crate) struct ParallelAgentTask {
    pub(crate) agent_id: String,
    pub(crate) prompt: String,
    #[serde(default)]
    pub(crate) context: Option<String>,
    #[serde(default)]
    pub(crate) toolkit: Option<String>,
    #[serde(default)]
    pub(crate) ownership: Option<String>,
    /// File-isolation strategy for this worker: `"none"` (default) or
    /// `"worktree"` (dedicated git worktree checkout).
    #[serde(default)]
    pub(crate) isolation: Option<String>,
    /// Worktree base ref: `"head"` (default) or `"fresh"`.
    #[serde(default)]
    pub(crate) base_ref: Option<String>,
}

/// Decode and validate the request batch before the live worker fanout.
///
/// This is the first real `validate`-node responsibility moved out of the tool
/// wrapper. Effectful worktree creation now runs in the graph path after the
/// action root and worker definitions are resolved.
fn validate_spawn_parallel_tasks(
    args: &serde_json::Value,
    max_parallel: Option<usize>,
) -> Result<Vec<ParallelAgentTask>, String> {
    let tasks_value = args
        .get("tasks")
        .cloned()
        .ok_or_else(|| "Missing 'tasks' parameter".to_string())?;
    let tasks: Vec<ParallelAgentTask> =
        serde_json::from_value(tasks_value).map_err(|e| format!("Invalid tasks array: {e}"))?;

    if tasks.len() < 2 {
        return Err("spawn_parallel_agents requires at least two tasks".to_string());
    }
    if let Some(max_parallel) = max_parallel {
        if tasks.len() > max_parallel {
            return Err(format!(
                "spawn_parallel_agents received {} tasks but max_parallel_tools is {}",
                tasks.len(),
                max_parallel
            ));
        }
    }

    Ok(tasks)
}

pub(crate) enum SpawnParallelTaskValidationError {
    MissingTasks(String),
    InvalidTasks(String),
    Rejected(String),
}

pub(crate) fn validate_spawn_parallel_tool_request(
    args: &serde_json::Value,
    max_parallel: Option<usize>,
) -> Result<Vec<ParallelAgentTask>, SpawnParallelTaskValidationError> {
    validate_spawn_parallel_tasks(args, max_parallel).map_err(|message| {
        if message == "Missing 'tasks' parameter" {
            SpawnParallelTaskValidationError::MissingTasks(message)
        } else if message.starts_with("Invalid tasks array:") {
            SpawnParallelTaskValidationError::InvalidTasks(message)
        } else {
            SpawnParallelTaskValidationError::Rejected(message)
        }
    })
}
