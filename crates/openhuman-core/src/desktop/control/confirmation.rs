//! One-use user decisions for Jev goal stops, separate from autonomy auto-approval.

use std::collections::HashMap;
use std::future::Future;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{json, Value};
use tinycomputer_bus::{names, DesktopResponse};

use crate::config::Config;

const TTL: Duration = Duration::from_secs(10 * 60);

#[derive(Clone)]
struct Pending {
    app: String,
    goal: String,
    origin: String,
    operation: String,
    target: Option<String>,
    target_name: Option<String>,
    target_role: String,
    action_summary: String,
    reason: String,
    expires_at: String,
    created: Instant,
    approved: bool,
    cancellation_in_flight: bool,
}

#[derive(Serialize)]
pub struct PendingDesktopConfirmation {
    pub confirmation_id: String,
    pub app: String,
    pub operation: String,
    pub target: Option<String>,
    pub target_name: Option<String>,
    pub target_role: String,
    pub action_summary: String,
    pub reason: String,
    pub expires_at: String,
    pub approved: bool,
}

fn table() -> &'static Mutex<HashMap<String, Pending>> {
    static TABLE: OnceLock<Mutex<HashMap<String, Pending>>> = OnceLock::new();
    TABLE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Capture only the operation and target named by a module confirmation stop.
pub(super) fn record(app: &str, goal: &str, thread_id: &str, data: &Value) {
    if data.get("stop").and_then(Value::as_str) != Some("confirmation_required") {
        return;
    }
    let Some(id) = data.get("confirmation_id").and_then(Value::as_str) else {
        return;
    };
    let Some(operation) = data
        .get("pending")
        .and_then(|value| value.get("operation"))
        .and_then(Value::as_str)
    else {
        return;
    };
    let target = data
        .get("pending")
        .and_then(|value| value.get("target"))
        .and_then(|value| value.get("ref_id"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    let target_name = data
        .get("pending")
        .and_then(|value| value.get("target"))
        .and_then(|value| value.get("name"))
        .and_then(Value::as_str)
        .and_then(|name| {
            (!name.trim().is_empty()).then(|| name.trim().chars().take(100).collect::<String>())
        });
    let target_role = data
        .get("pending")
        .and_then(|value| value.get("target"))
        .and_then(|value| value.get("role"))
        .and_then(Value::as_str)
        .and_then(|role| {
            (!role.trim().is_empty()).then(|| role.trim().chars().take(60).collect::<String>())
        });
    // The raw ref cannot tell a person which control will be activated.
    let (Some(target_name), Some(target_role)) = (target_name, target_role) else {
        return;
    };
    let action_summary = format!("{operation} {target_role} '{target_name}' in {app}");
    let reason = data
        .get("pending")
        .and_then(|value| value.get("reason"))
        .and_then(Value::as_str)
        .unwrap_or("This desktop action needs confirmation.")
        .to_owned();
    let expires_at = (chrono::Utc::now() + chrono::Duration::minutes(10)).to_rfc3339();
    table()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(
            id.to_owned(),
            Pending {
                app: app.to_owned(),
                goal: goal.to_owned(),
                origin: thread_id.to_owned(),
                operation: operation.to_owned(),
                target,
                target_name: Some(target_name),
                target_role,
                action_summary,
                reason,
                expires_at,
                created: Instant::now(),
                approved: false,
                cancellation_in_flight: false,
            },
        );
}

pub fn pending() -> Vec<PendingDesktopConfirmation> {
    if !super::ops::listener_is_loopback() {
        return Vec::new();
    }
    let mut guard = table()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    guard.retain(|_, value| value.created.elapsed() < TTL);
    guard
        .iter()
        .map(|(id, value)| PendingDesktopConfirmation {
            confirmation_id: id.clone(),
            app: value.app.clone(),
            operation: value.operation.clone(),
            target: value.target.clone(),
            approved: value.approved,
            target_name: value.target_name.clone(),
            target_role: value.target_role.clone(),
            action_summary: value.action_summary.clone(),
            reason: value.reason.clone(),
            expires_at: value.expires_at.clone(),
        })
        .collect()
}

/// Called only by a trusted RPC client carrying the core launch bearer.
pub async fn confirm(config: &Config, id: &str, approve: bool) -> Result<Value, String> {
    confirm_with(id, approve, |app, goal, confirmation_id| async move {
        crate::modules::desktop::call(
            config,
            names::methods::RUN_GOAL,
            json!({"app":app,"goal":goal,
                "continuation":{"id":confirmation_id,"approve":false}}),
        )
        .await
    })
    .await
}

async fn confirm_with<F, Fut>(id: &str, approve: bool, cancel: F) -> Result<Value, String>
where
    F: FnOnce(String, String, String) -> Fut,
    Fut: Future<Output = Result<DesktopResponse, String>>,
{
    if !super::ops::listener_is_loopback() {
        return Err("desktop confirmation requires a loopback core listener".to_owned());
    }
    let (app, goal, created) = {
        let mut guard = table()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        guard.retain(|_, value| value.created.elapsed() < TTL);
        let item = guard
            .get_mut(id)
            .ok_or("desktop confirmation is missing or expired")?;
        if item.cancellation_in_flight {
            return Err("desktop cancellation is already in progress".to_owned());
        }
        if approve {
            item.approved = true;
            return Ok(json!({"confirmation_id":id,"approve":true}));
        }
        item.approved = false;
        item.cancellation_in_flight = true;
        (item.app.clone(), item.goal.clone(), item.created)
    };
    let result = cancel(app, goal, id.to_owned()).await;
    let succeeded = result.as_ref().is_ok_and(|reply| {
        reply.ok
            && reply
                .data
                .as_ref()
                .and_then(|data| data.get("stop"))
                .and_then(Value::as_str)
                == Some("cancelled")
    });
    {
        let mut guard = table()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if guard.get(id).is_some_and(|item| item.created == created) {
            if succeeded {
                guard.remove(id);
            } else if let Some(item) = guard.get_mut(id) {
                item.cancellation_in_flight = false;
            }
        }
    }
    let reply = result?;
    if !reply.ok {
        return Err(reply.error.map_or_else(
            || "desktop cancellation failed".to_owned(),
            |error| error.message,
        ));
    }
    if !succeeded {
        return Err("desktop cancellation was not confirmed".to_owned());
    }
    Ok(json!({"confirmation_id":id,"approve":false}))
}

/// Consume an approved decision before making one continuation call.
pub(super) fn take_approved(id: &str, thread_id: Option<&str>) -> Result<(String, String), String> {
    let thread_id = thread_id
        .filter(|thread_id| !thread_id.is_empty())
        .ok_or("desktop confirmation requires a threaded agent run")?;
    let mut guard = table()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let item = guard
        .get(id)
        .ok_or("desktop confirmation is missing or expired")?;
    if item.created.elapsed() >= TTL {
        guard.remove(id);
        return Err("desktop confirmation expired".to_owned());
    }
    if item.cancellation_in_flight {
        return Err("desktop cancellation is already in progress".to_owned());
    }
    if !item.approved {
        return Err("desktop action needs the user's explicit confirmation".to_owned());
    }
    if item.origin != thread_id {
        return Err("desktop confirmation does not match this thread".to_owned());
    }
    let item = guard.remove(id).expect("checked above");
    Ok((item.app, item.goal))
}

#[cfg(test)]
#[path = "confirmation_tests.rs"]
mod tests;
