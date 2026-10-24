//! OpenHuman's `RunQueue` fallback for TinyAgents detached steering.

use tinyagents_harness::ids::TaskId;
use tinyagents_harness::run_queue::QueueLane;
use tinyagents_orchestration::steering::{
    queue_lane_name, steer_detached, SteerAccess, SteerError,
};

use super::cancel::now_ms;
use super::registry::{control_admits, owner_key, registry, RunningSubagentMetadata};

/// Inject an instruction into a detached sub-agent owned by `parent_session`.
pub async fn steer(
    ctx: &crate::core::runtime::DispatchCtx,
    task_id: &str,
    parent_session: &str,
    text: String,
    lane: QueueLane,
) -> Result<(), SteerError> {
    let owner = owner_key(ctx, parent_session).map_err(|_| SteerError::NotOwned)?;
    let route = steer_detached(
        registry(),
        task_id,
        SteerAccess::Owner(&owner),
        text,
        lane,
        |metadata: RunningSubagentMetadata, lane, text| async move {
            metadata
                .run_queue
                .push(
                    lane,
                    crate::agent::queued_turn::QueuedTurn {
                        id: uuid::Uuid::new_v4().to_string(),
                        text,
                        client_id: "steer_subagent".to_string(),
                        thread_id: task_id.to_string(),
                        queued_at_ms: now_ms(),
                        model_override: None,
                        temperature: None,
                        locale: None,
                    },
                )
                .await;
        },
    )
    .await?;
    tracing::info!(
        task_id,
        lane = queue_lane_name(lane),
        ?route,
        "[running_subagents] steered detached task"
    );
    Ok(())
}

/// User-control variant for JSON-RPC sub-agent controls.
///
/// The caller's profile is checked against the task metadata before using the
/// trusted upstream route. Desktop controls retain authority over process
/// workers, while SaaS controls remain scoped to their captured profile.
pub(crate) async fn steer_control(
    ctx: &crate::core::runtime::DispatchCtx,
    task_id: &str,
    text: String,
    lane: QueueLane,
) -> Result<(), SteerError> {
    let key = TaskId::new(task_id);
    let snapshot = registry()
        .snapshot_trusted(&key)
        .map_err(SteerError::from)?;
    if !control_admits(ctx, &snapshot.metadata) {
        return Err(SteerError::NotOwned);
    }
    let route = steer_detached(
        registry(),
        task_id,
        SteerAccess::Trusted,
        text,
        lane,
        |metadata: RunningSubagentMetadata, lane, text| async move {
            metadata
                .run_queue
                .push(
                    lane,
                    crate::agent::queued_turn::QueuedTurn {
                        id: uuid::Uuid::new_v4().to_string(),
                        text,
                        client_id: "subagent_control_rpc".to_string(),
                        thread_id: task_id.to_string(),
                        queued_at_ms: now_ms(),
                        model_override: None,
                        temperature: None,
                        locale: None,
                    },
                )
                .await;
        },
    )
    .await?;
    tracing::info!(
        task_id,
        lane = queue_lane_name(lane),
        ?route,
        "[running_subagents] control-steered detached task"
    );
    Ok(())
}
