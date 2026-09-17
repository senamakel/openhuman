//! Thin JSON-RPC handlers that parse params and delegate to
//! [`super::super::ops`] and [`super::super::runs`].

use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{Map, Value};

use crate::agent::task_board::{TaskApprovalMode, TaskBoardCard};
use crate::core::all::ControllerFuture;

use super::super::ops::{self, BoardLocation, CardPatch, TodosSnapshot};
use super::super::runs;

#[derive(Debug, Deserialize)]
struct ThreadIdParams {
    thread_id: String,
}

#[derive(Debug, Deserialize)]
struct AddParams {
    thread_id: String,
    content: String,
    #[serde(default, alias = "sourceMetadata")]
    source_metadata: Option<Value>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    objective: Option<String>,
    #[serde(default)]
    plan: Option<Vec<String>>,
    #[serde(default, alias = "assignedAgent")]
    assigned_agent: Option<String>,
    #[serde(default)]
    #[serde(alias = "allowedTools")]
    allowed_tools: Option<Vec<String>>,
    #[serde(default)]
    #[serde(alias = "approvalMode")]
    approval_mode: Option<String>,
    #[serde(default)]
    #[serde(alias = "acceptanceCriteria")]
    acceptance_criteria: Option<Vec<String>>,
    #[serde(default)]
    evidence: Option<Vec<String>>,
    #[serde(default)]
    notes: Option<String>,
    #[serde(default)]
    blocker: Option<String>,
}

#[derive(Debug, Deserialize)]
struct EditParams {
    thread_id: String,
    id: String,
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    objective: Option<String>,
    #[serde(default)]
    plan: Option<Vec<String>>,
    #[serde(default, alias = "assignedAgent")]
    assigned_agent: Option<String>,
    #[serde(default)]
    #[serde(alias = "allowedTools")]
    allowed_tools: Option<Vec<String>>,
    #[serde(default)]
    #[serde(alias = "acceptanceCriteria")]
    acceptance_criteria: Option<Vec<String>>,
    #[serde(default)]
    evidence: Option<Vec<String>>,
    #[serde(default)]
    notes: Option<String>,
    #[serde(default)]
    blocker: Option<String>,
}

#[derive(Debug, Deserialize)]
struct UpdateStatusParams {
    thread_id: String,
    id: String,
    status: String,
}

#[derive(Debug, Deserialize)]
struct RemoveParams {
    thread_id: String,
    id: String,
}

#[derive(Debug, Deserialize)]
struct SetSessionThreadParams {
    thread_id: String,
    id: String,
    #[serde(default, alias = "sessionThreadId")]
    session_thread_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DecidePlanParams {
    thread_id: String,
    id: String,
    approve: bool,
}

#[derive(Debug, Deserialize)]
struct RevisePlanParams {
    thread_id: String,
    #[serde(default)]
    feedback: String,
}

#[derive(Debug, Deserialize)]
struct ReplaceParams {
    thread_id: String,
    cards: Vec<TaskBoardCard>,
}

pub(super) fn handle_list(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let p = parse::<ThreadIdParams>(params)?;
        let loc = thread_location(&p.thread_id).await?;
        tracing::debug!(thread_id = %p.thread_id, "[rpc][todos] list entry");
        snapshot_to_json(ops::list(&loc).await?)
    })
}

pub(super) fn handle_add(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let p = parse::<AddParams>(params)?;
        let loc = thread_location(&p.thread_id).await?;
        let patch = CardPatch {
            content: None,
            status: p.status.as_deref().map(ops::parse_status).transpose()?,
            objective: p.objective,
            plan: p.plan,
            assigned_agent: p.assigned_agent,
            allowed_tools: p.allowed_tools,
            approval_mode: Some(parse_approval_mode(p.approval_mode)?),
            acceptance_criteria: p.acceptance_criteria,
            evidence: p.evidence,
            notes: p.notes,
            blocker: p.blocker,
            // Carry the originating source identifiers onto the promoted card so
            // the inbox can tell an item was already picked up (dedup/hide).
            source_metadata: p.source_metadata,
        };
        tracing::debug!(thread_id = %p.thread_id, "[rpc][todos] add entry");
        snapshot_to_json(ops::add(&loc, &p.content, patch).await?)
    })
}

pub(super) fn handle_edit(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let approval_mode = approval_mode_patch_from_params(&params)?;
        let p = parse::<EditParams>(params)?;
        let loc = thread_location(&p.thread_id).await?;
        let patch = CardPatch {
            content: p.content,
            status: p.status.as_deref().map(ops::parse_status).transpose()?,
            objective: p.objective,
            plan: p.plan,
            assigned_agent: p.assigned_agent,
            allowed_tools: p.allowed_tools,
            approval_mode,
            acceptance_criteria: p.acceptance_criteria,
            evidence: p.evidence,
            notes: p.notes,
            blocker: p.blocker,
            source_metadata: None,
        };
        tracing::debug!(thread_id = %p.thread_id, id = %p.id, "[rpc][todos] edit entry");
        snapshot_to_json(ops::edit(&loc, &p.id, patch).await?)
    })
}

pub(super) fn handle_update_status(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let p = parse::<UpdateStatusParams>(params)?;
        let loc = thread_location(&p.thread_id).await?;
        let status = ops::parse_status(&p.status)?;
        tracing::debug!(
            thread_id = %p.thread_id,
            id = %p.id,
            status = %p.status,
            "[rpc][todos] update_status entry"
        );
        snapshot_to_json(ops::update_status(&loc, &p.id, status).await?)
    })
}

pub(super) fn handle_set_session_thread(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let p = parse::<SetSessionThreadParams>(params)?;
        let loc = thread_location(&p.thread_id).await?;
        tracing::debug!(
            thread_id = %p.thread_id,
            id = %p.id,
            session_thread_id = ?p.session_thread_id,
            "[rpc][todos] set_session_thread entry"
        );
        snapshot_to_json(ops::set_session_thread(&loc, &p.id, p.session_thread_id).await?)
    })
}

pub(super) fn handle_decide_plan(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let p = parse::<DecidePlanParams>(params)?;
        let loc = thread_location(&p.thread_id).await?;
        tracing::debug!(
            thread_id = %p.thread_id,
            id = %p.id,
            approve = p.approve,
            "[rpc][todos] decide_plan entry"
        );
        snapshot_to_json(ops::decide_plan(&loc, &p.id, p.approve).await?)
    })
}

pub(super) fn handle_revise_plan(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let p = parse::<RevisePlanParams>(params)?;
        let loc = thread_location(&p.thread_id).await?;
        tracing::debug!(
            thread_id = %p.thread_id,
            feedback_len = p.feedback.len(),
            "[rpc][todos] revise_plan entry"
        );
        snapshot_to_json(ops::revise_plan(&loc, &p.feedback).await?)
    })
}

fn parse_approval_mode(raw: Option<String>) -> Result<Option<TaskApprovalMode>, String> {
    let Some(raw) = raw else {
        return Ok(None);
    };
    match raw.trim() {
        "required" => Ok(Some(TaskApprovalMode::Required)),
        "not_required" => Ok(Some(TaskApprovalMode::NotRequired)),
        other => Err(format!(
            "invalid approval_mode '{other}' (expected required|not_required)"
        )),
    }
}

fn approval_mode_patch_from_params(
    params: &Map<String, Value>,
) -> Result<Option<Option<TaskApprovalMode>>, String> {
    let Some(value) = params
        .get("approvalMode")
        .or_else(|| params.get("approval_mode"))
    else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(Some(None));
    }
    let Some(raw) = value.as_str() else {
        return Err("invalid approval_mode type (expected required|not_required|null)".to_string());
    };
    parse_approval_mode(Some(raw.to_string())).map(Some)
}

pub(super) fn handle_remove(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let p = parse::<RemoveParams>(params)?;
        let loc = thread_location(&p.thread_id).await?;
        tracing::debug!(thread_id = %p.thread_id, id = %p.id, "[rpc][todos] remove entry");
        snapshot_to_json(ops::remove(&loc, &p.id).await?)
    })
}

pub(super) fn handle_replace(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let p = parse::<ReplaceParams>(params)?;
        let loc = thread_location(&p.thread_id).await?;
        tracing::debug!(
            thread_id = %p.thread_id,
            card_count = p.cards.len(),
            "[rpc][todos] replace entry"
        );
        snapshot_to_json(ops::replace(&loc, p.cards).await?)
    })
}

pub(super) fn handle_clear(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let p = parse::<ThreadIdParams>(params)?;
        let loc = thread_location(&p.thread_id).await?;
        tracing::debug!(thread_id = %p.thread_id, "[rpc][todos] clear entry");
        snapshot_to_json(ops::clear(&loc).await?)
    })
}

// ── run handlers ────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct RunListParams {
    thread_id: String,
    #[serde(default, alias = "cardId")]
    card_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RunGetParams {
    thread_id: String,
    #[serde(alias = "runId")]
    run_id: String,
}

#[derive(Debug, Deserialize)]
struct ReclaimStaleParams {
    thread_id: String,
    #[serde(default, alias = "heartbeatStaleSecs")]
    heartbeat_stale_secs: Option<u64>,
    #[serde(default, alias = "claimTtlSecs")]
    claim_ttl_secs: Option<u64>,
    #[serde(default, alias = "maxReclaimCount")]
    max_reclaim_count: Option<u32>,
}

pub(super) fn handle_run_list(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let p = parse::<RunListParams>(params)?;
        let loc = thread_location(&p.thread_id).await?;
        tracing::debug!(
            thread_id = %p.thread_id,
            card_id = ?p.card_id,
            "[rpc][todos] run_list entry"
        );
        let run_list = runs::list_runs(&loc, p.card_id.as_deref()).await?;
        serde_json::to_value(&run_list).map_err(|e| format!("serialize runs: {e}"))
    })
}

pub(super) fn handle_run_get(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let p = parse::<RunGetParams>(params)?;
        let loc = thread_location(&p.thread_id).await?;
        tracing::debug!(
            thread_id = %p.thread_id,
            run_id = %p.run_id,
            "[rpc][todos] run_get entry"
        );
        let run = runs::get_run(&loc, &p.run_id).await?;
        serde_json::to_value(&run).map_err(|e| format!("serialize run: {e}"))
    })
}

pub(super) fn handle_reclaim_stale(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let p = parse::<ReclaimStaleParams>(params)?;
        let loc = thread_location(&p.thread_id).await?;
        let limits = runs::RunLimits {
            heartbeat_stale_secs: p
                .heartbeat_stale_secs
                .unwrap_or(runs::DEFAULT_HEARTBEAT_STALE_SECS),
            claim_ttl_secs: p.claim_ttl_secs.unwrap_or(runs::DEFAULT_CLAIM_TTL_SECS),
            max_reclaim_count: p
                .max_reclaim_count
                .unwrap_or(runs::DEFAULT_MAX_RECLAIM_COUNT),
        };
        tracing::debug!(
            thread_id = %p.thread_id,
            ?limits,
            "[rpc][todos] reclaim_stale entry"
        );
        let result = runs::reclaim_stale(&loc, &limits).await?;
        serde_json::to_value(&result).map_err(|e| format!("serialize reclaim result: {e}"))
    })
}

// ── helpers ──────────────────────────────────────────────────────────

async fn thread_location(thread_id: &str) -> Result<BoardLocation, String> {
    let trimmed = thread_id.trim();
    if trimmed.is_empty() {
        return Err("thread_id must not be empty".to_string());
    }
    let config = crate::config::Config::load_or_init()
        .await
        .map_err(|e| format!("load config: {e}"))?;
    Ok(BoardLocation::Thread {
        workspace_dir: config.workspace_dir,
        thread_id: trimmed.to_string(),
    })
}

fn parse<T: DeserializeOwned>(params: Map<String, Value>) -> Result<T, String> {
    serde_json::from_value(Value::Object(params)).map_err(|e| format!("invalid params: {e}"))
}

fn snapshot_to_json(snap: TodosSnapshot) -> Result<Value, String> {
    serde_json::to_value(&snap).map_err(|e| format!("serialize snapshot: {e}"))
}
