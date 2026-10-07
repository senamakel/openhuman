//! Argument parsing and host approval for browser tool actions.

use super::Pending;
use crate::security::approval::{ApprovalGate, GateOutcome};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use tinycomputer_bus::agent::{TaskStatus, TaskView};
use tinycomputer_bus::browser::{Action, LocateBy, Locator, ScrollDirection, Target, WaitState};

/// What the host adds to a task report the agent cannot act on alone: a
/// task that failed because Chrome was not found says where the user sets
/// its path, rather than leaving the agent to install a browser.
pub(super) fn host_hint(view: &TaskView) -> Option<&'static str> {
    match &view.status {
        TaskStatus::Failed { reason, hint, .. }
            if crate::modules::browser::chrome_not_found(reason)
                || crate::modules::browser::chrome_not_found(hint) =>
        {
            Some(crate::modules::browser::CHROME_NOT_FOUND_HINT)
        }
        _ => None,
    }
}

pub(super) fn task_inputs(args: &Value) -> anyhow::Result<BTreeMap<String, String>> {
    args["inputs"].as_object().map_or_else(
        || Ok(BTreeMap::new()),
        |inputs| {
            inputs
                .iter()
                .map(|(k, v)| {
                    Ok((
                        k.clone(),
                        v.as_str()
                            .ok_or_else(|| anyhow::anyhow!("Task input '{k}' must be text"))?
                            .to_owned(),
                    ))
                })
                .collect()
        },
    )
}

/// Ask the host approval gate about a paused task's exact action. A missing
/// gate denies: a task never takes an irreversible step unapproved, unless a
/// trusted unattended turn has `task_step` in `[browser] unattended_actions`.
pub(super) async fn approve_task_action(
    pending: &Pending,
    browser: &crate::config::BrowserConfig,
    origin: Option<&crate::agent::turn_origin::AgentTurnOrigin>,
) -> anyhow::Result<bool> {
    let clean = |raw: &str| {
        let cleaned = raw.chars().filter(|c| !c.is_control()).collect::<String>();
        let mut short = cleaned.chars().take(160).collect::<String>();
        if cleaned.chars().count() > 160 {
            short.push('…');
        }
        short
    };
    let digest = Sha256::digest(serde_json::to_vec(&json!({
        "task": pending.task, "action": pending.action, "target": pending.target
    }))?);
    let digest_hex = format!("{digest:x}");
    if super::unattended::allow(origin, browser, "task_step", &digest_hex) {
        return Ok(true);
    }
    let gate = ApprovalGate::try_global().ok_or_else(|| {
        anyhow::anyhow!("[policy-denied] Browser action needs an interactive host approval gate")
    })?;
    let summary = format!(
        "Browser task: {} — {} [action {}]",
        clean(&pending.action),
        clean(&pending.target),
        &digest_hex[..12]
    );
    let args = json!({"action": "task_step", "target": summary, "exact_action_sha256": digest_hex});
    Ok(
        match gate.intercept_forced("browser", &summary, args).await {
            GateOutcome::Allow => true,
            GateOutcome::Deny { reason } => {
                tracing::debug!(%reason, "[browser] task action denied by host");
                false
            }
        },
    )
}

pub(super) fn required<'a>(args: &'a Value, key: &str) -> anyhow::Result<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| anyhow::anyhow!("Missing '{key}' parameter"))
}

pub(super) fn parse_action(args: &Value) -> anyhow::Result<Action> {
    let target = || required(args, "selector").map(Target::parse);
    Ok(match required(args, "action")? {
        "click" => Action::Click {
            target: target()?,
            new_tab: false,
        },
        "fill" => Action::Fill {
            target: target()?,
            value: required(args, "value")?.into(),
        },
        "type" => Action::Type {
            target: args["selector"].as_str().map(Target::parse),
            text: required(args, "text")?.into(),
            delay_ms: None,
        },
        "get_text" => Action::GetText { target: target()? },
        "is_visible" => Action::IsVisible { target: target()? },
        "hover" => Action::Hover { target: target()? },
        "press" => Action::Press {
            key: required(args, "key")?.into(),
        },
        "scroll" => Action::Scroll {
            direction: match required(args, "direction")? {
                "up" => ScrollDirection::Up,
                "down" => ScrollDirection::Down,
                "left" => ScrollDirection::Left,
                "right" => ScrollDirection::Right,
                x => anyhow::bail!("Invalid direction: {x}"),
            },
            pixels: args["pixels"].as_u64().and_then(|v| u32::try_from(v).ok()),
            target: None,
        },
        "wait" => Action::WaitFor {
            target: args["selector"].as_str().map(Target::parse),
            text: args["text"].as_str().map(str::to_owned),
            state: WaitState::Visible,
            ms: args["ms"].as_u64(),
            timeout_ms: args["timeout_ms"].as_u64(),
        },
        "find" => {
            let by = match required(args, "by")? {
                "role" => LocateBy::Role,
                "text" => LocateBy::Text,
                "label" => LocateBy::Label,
                "placeholder" => LocateBy::Placeholder,
                "testid" => LocateBy::TestId,
                x => anyhow::bail!("Invalid locator: {x}"),
            };
            let target = Target::locator(Locator::new(by, required(args, "value")?));
            match required(args, "find_action")? {
                "click" => Action::Click {
                    target,
                    new_tab: false,
                },
                "fill" => Action::Fill {
                    target,
                    value: required(args, "fill_value")?.into(),
                },
                "text" => Action::GetText { target },
                "hover" => Action::Hover { target },
                x => anyhow::bail!("Invalid find action: {x}"),
            }
        }
        x => anyhow::bail!("Unsupported browser action: {x}"),
    })
}
