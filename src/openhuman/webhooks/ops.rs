use crate::openhuman::skills::global_engine;
use crate::openhuman::webhooks::{
    WebhookDebugLogListResult, WebhookDebugLogsClearedResult, WebhookDebugRegistrationsResult,
};
use crate::rpc::RpcOutcome;

pub async fn list_registrations() -> Result<RpcOutcome<WebhookDebugRegistrationsResult>, String> {
    let engine = global_engine().ok_or_else(|| "skill runtime not initialized".to_string())?;
    let registrations = engine.webhook_router().list_all();
    let count = registrations.len();

    Ok(RpcOutcome::single_log(
        WebhookDebugRegistrationsResult { registrations },
        format!("webhooks.list_registrations returned {count} registration(s)"),
    ))
}

pub async fn list_logs(
    limit: Option<usize>,
) -> Result<RpcOutcome<WebhookDebugLogListResult>, String> {
    let engine = global_engine().ok_or_else(|| "skill runtime not initialized".to_string())?;
    let logs = engine.webhook_router().list_logs(limit);
    let count = logs.len();

    Ok(RpcOutcome::single_log(
        WebhookDebugLogListResult { logs },
        format!("webhooks.list_logs returned {count} log entrie(s)"),
    ))
}

pub async fn clear_logs() -> Result<RpcOutcome<WebhookDebugLogsClearedResult>, String> {
    let engine = global_engine().ok_or_else(|| "skill runtime not initialized".to_string())?;
    let cleared = engine.webhook_router().clear_logs();

    Ok(RpcOutcome::single_log(
        WebhookDebugLogsClearedResult { cleared },
        format!("webhooks.clear_logs removed {cleared} log entrie(s)"),
    ))
}
