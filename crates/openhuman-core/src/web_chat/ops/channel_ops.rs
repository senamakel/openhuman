//! Public RPC-facing operations for the web channel: sending a chat message,
//! cancelling in-flight turns (primary and parallel, scoped or unscoped), and
//! inspecting/clearing a thread's run queue.

use serde_json::{json, Value};

use crate::core::socketio::WebChannelEvent;
use crate::rpc::RpcOutcome;

use super::super::event_bus::publish_web_channel_event;
use super::super::types::ChatRequestMetadata;
use super::parallel_turn::{cancel_parallel_turn_by_request_id, cancel_parallel_turns_for_thread};
use super::start_chat::start_chat;
use super::state::{cancel_in_flight_gracefully, cancel_should_target, key_for, IN_FLIGHT};

/// Cancel whatever turn is currently running on a thread (unscoped stop).
///
/// Back-compat entry point (Stop button / session teardown). For a cancel that
/// must only affect a specific turn — so a stale cancel can't kill a newer turn
/// on the same thread — use [`cancel_chat_scoped`] with the target `request_id`
/// (#4760).
pub async fn cancel_chat(client_id: &str, thread_id: &str) -> Result<Option<String>, String> {
    cancel_chat_scoped(client_id, thread_id, None).await
}

/// Cancel the in-flight turn(s) for a thread.
///
/// When `request_id` is `Some`, the cancel is **scoped**: it only tears down the
/// primary turn if that exact request is still running (and only the matching
/// parallel turn), so a stale cancel for a superseded request can't kill the
/// newer turn that replaced it (#4760). When `request_id` is `None`, it stops
/// whatever is running on the thread (primary + every parallel) — the "stop
/// everything" behaviour used by session teardown / a Stop button.
pub async fn cancel_chat_scoped(
    client_id: &str,
    thread_id: &str,
    request_id: Option<&str>,
) -> Result<Option<String>, String> {
    let client_id = client_id.trim();
    let thread_id = thread_id.trim();

    if client_id.is_empty() {
        return Err("client_id is required".to_string());
    }
    if thread_id.is_empty() {
        return Err("thread_id is required".to_string());
    }

    let map_key = key_for(thread_id);
    let mut removed_request_id: Option<String> = None;

    {
        let mut in_flight = IN_FLIGHT.lock().await;
        // #4760: only tear down the primary turn when the cancel is unscoped OR
        // targets exactly the request that is running. A stale cancel for an
        // already-superseded request must be a no-op so the newer turn lives.
        let should_cancel_primary = in_flight
            .get(&map_key)
            .map(|entry| cancel_should_target(request_id, &entry.request_id))
            .unwrap_or(false);
        if should_cancel_primary {
            if let Some(existing) = in_flight.remove(&map_key) {
                removed_request_id = Some(cancel_in_flight_gracefully(existing));
            }
        } else if let Some(rid) = request_id {
            log::info!(
                "[web-channel] ignoring stale cancel request_id={} for thread_id={} — current in-flight is {:?}; newer turn preserved",
                rid,
                thread_id,
                in_flight.get(&map_key).map(|e| e.request_id.as_str())
            );
        }
    }

    // Also tear down concurrent parallel (forked) turns. A scoped cancel targets
    // only the named parallel turn (if it is one); an unscoped cancel/stop
    // covers every parallel turn on the thread, not just the primary one.
    let cancelled_parallel = match request_id {
        Some(rid) => cancel_parallel_turn_by_request_id(thread_id, rid).await,
        None => cancel_parallel_turns_for_thread(thread_id).await,
    };

    // #4760: a scoped cancel that matched only a parallel (forked) turn — not the
    // primary — still genuinely tore a turn down and emitted its cancelled event.
    // Surface that id so `channel_web_cancel` reports `cancelled: true` with the
    // right request_id instead of misreporting a no-op just because the primary
    // turn wasn't the one cancelled.
    let cancelled_any = removed_request_id
        .clone()
        .or_else(|| cancelled_parallel.first().cloned());

    // Emit a cancelled chat_error for each cancelled turn (primary + parallels)
    // so every interleaved branch's UI is resolved.
    for request_id in removed_request_id.into_iter().chain(cancelled_parallel) {
        publish_web_channel_event(WebChannelEvent {
            event: "chat_error".to_string(),
            client_id: client_id.to_string(),
            thread_id: thread_id.to_string(),
            request_id,
            message: Some("Cancelled".to_string()),
            error_type: Some("cancelled".to_string()),
            ..Default::default()
        });
    }

    Ok(cancelled_any)
}

pub async fn channel_web_chat(
    client_id: &str,
    thread_id: &str,
    message: &str,
    model_override: Option<String>,
    temperature: Option<f64>,
    profile_id: Option<String>,
    locale: Option<String>,
    queue_mode: Option<String>,
    metadata: ChatRequestMetadata,
) -> Result<RpcOutcome<Value>, String> {
    let result = start_chat(
        client_id,
        thread_id,
        message,
        model_override,
        temperature,
        profile_id,
        locale,
        queue_mode,
        metadata,
    )
    .await?;

    if let Ok(parsed) = serde_json::from_str::<Value>(&result) {
        return Ok(RpcOutcome::single_log(parsed, "web channel message queued"));
    }

    Ok(RpcOutcome::single_log(
        json!({
            "accepted": true,
            "client_id": client_id.trim(),
            "thread_id": thread_id.trim(),
            "request_id": result,
        }),
        "web channel request accepted",
    ))
}

pub async fn channel_web_queue_status(thread_id: &str) -> Result<RpcOutcome<Value>, String> {
    let map_key = key_for(thread_id);
    let in_flight = IN_FLIGHT.lock().await;
    if let Some(entry) = in_flight.get(&map_key) {
        let status = entry.run_queue.status().await;
        Ok(RpcOutcome::single_log(
            json!({
                "thread_id": thread_id.trim(),
                "active": true,
                "request_id": entry.request_id,
                "steers": status.steers,
                "followups": status.followups,
                "collects": status.collects,
                "total": status.total,
            }),
            "queue status retrieved",
        ))
    } else {
        Ok(RpcOutcome::single_log(
            json!({
                "thread_id": thread_id.trim(),
                "active": false,
                "steers": 0,
                "followups": 0,
                "collects": 0,
                "total": 0,
            }),
            "no active turn for thread",
        ))
    }
}

pub async fn channel_web_queue_clear(thread_id: &str) -> Result<RpcOutcome<Value>, String> {
    let map_key = key_for(thread_id);
    let in_flight = IN_FLIGHT.lock().await;
    if let Some(entry) = in_flight.get(&map_key) {
        let dropped = entry.run_queue.clear().await;
        log::info!(
            "[web-channel] cleared queue thread_id={} dropped={}",
            thread_id,
            dropped
        );
        Ok(RpcOutcome::single_log(
            json!({
                "thread_id": thread_id.trim(),
                "cleared": true,
                "dropped": dropped,
            }),
            "queue cleared",
        ))
    } else {
        Ok(RpcOutcome::single_log(
            json!({
                "thread_id": thread_id.trim(),
                "cleared": false,
                "dropped": 0,
            }),
            "no active turn for thread",
        ))
    }
}

pub async fn channel_web_cancel(
    client_id: &str,
    thread_id: &str,
    request_id: Option<&str>,
) -> Result<RpcOutcome<Value>, String> {
    let cancelled_request_id = cancel_chat_scoped(client_id, thread_id, request_id).await?;

    // No web-channel turn matched. Fall through to the task-dispatcher registry,
    // which holds autonomous runs that are NOT web-channel turns (so they never
    // appear in IN_FLIGHT and can only be reached here). The fallback is itself
    // request-scoped: a scoped cancel aborts the run only when its run_id
    // matches, so a stale cancel for a superseded request can't tear down a newer
    // run on the thread (#4760); an unscoped stop aborts whatever run is running.
    let cancelled = if cancelled_request_id.is_some() {
        true
    } else {
        crate::agent::task_dispatcher::cancel_session_scoped(thread_id.trim(), request_id).await
    };

    Ok(RpcOutcome::single_log(
        json!({
            "cancelled": cancelled,
            "client_id": client_id.trim(),
            "thread_id": thread_id.trim(),
            "request_id": cancelled_request_id,
        }),
        "web channel cancellation processed",
    ))
}
