//! `memory_tree_set_enabled` (#1856 Part 1): the single-field toggle for the
//! scheduler-gate mode.

use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::rpc::RpcOutcome;

/// Request shape for `memory_tree_set_enabled`. Single field — the caller
/// asks to enable (auto-mode) or pause (off-mode) all LLM-bound background
/// work.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SetEnabledRequest {
    /// `true` ⇒ scheduler-gate mode becomes `auto`. `false` ⇒ `off`.
    pub enabled: bool,
}

/// Response shape for `memory_tree_set_enabled`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SetEnabledResponse {
    /// Echo of the requested `enabled` state (post-write).
    pub enabled: bool,
    /// `true` when the saved mode actually flipped; `false` for no-ops.
    pub changed: bool,
    /// New scheduler-gate mode as wire string (`auto` / `off`).
    pub mode: String,
}

/// `memory_tree_set_enabled` RPC handler (#1856 Part 1).
///
/// Flips `config.scheduler_gate().mode` to either `Auto` (enabled) or `Off`
/// (paused), persists to disk via `config.save()`, and hot-reloads the
/// live scheduler-gate state so any in-flight workers immediately observe
/// the new policy at their next `wait_for_capacity()` await.
///
/// Notes:
/// - This is intentionally a single-field RPC (no batched
///   `MemoryTreeSettingsPatch`) — keeps the surface tight while #1856
///   Part 2 work lands the broader settings story.
/// - The 20-min Composio fetch loop is *not* paused by this toggle yet —
///   that requires a separate `Notify` signal and is queued for Part 2.
pub async fn set_enabled_rpc(
    config: &mut Config,
    req: SetEnabledRequest,
) -> Result<RpcOutcome<SetEnabledResponse>, String> {
    use tinymemory_api::host::SchedulerGateMode;

    let prev_mode = config.scheduler_gate.mode;
    let new_mode = if req.enabled {
        SchedulerGateMode::Auto
    } else {
        SchedulerGateMode::Off
    };

    log::debug!(
        "[memory-tree][rpc] set_enabled: requested enabled={} prev_mode={} new_mode={}",
        req.enabled,
        prev_mode.as_str(),
        new_mode.as_str(),
    );

    if prev_mode == new_mode {
        log::info!(
            "[memory-tree][rpc] set_enabled: no-op (mode already {})",
            new_mode.as_str()
        );
        return Ok(RpcOutcome::single_log(
            SetEnabledResponse {
                enabled: req.enabled,
                changed: false,
                mode: new_mode.as_str().to_string(),
            },
            format!(
                "memory_tree: set_enabled no-op enabled={} mode={}",
                req.enabled,
                new_mode.as_str()
            ),
        ));
    }

    config.scheduler_gate.mode = new_mode;
    config.save().await.map_err(|e| {
        let msg = format!("set_enabled: config.save failed: {e}");
        log::warn!("[memory-tree][rpc] {msg}");
        msg
    })?;

    // Hot-reload the live gate state — workers re-poll inside
    // `wait_for_capacity` and pick up the new policy without a restart.
    crate::cron::scheduler_gate::gate::update_config(config.scheduler_gate.clone());

    log::info!(
        "[memory-tree][rpc] set_enabled: scheduler_gate.mode {} -> {} (enabled={})",
        prev_mode.as_str(),
        new_mode.as_str(),
        req.enabled,
    );

    Ok(RpcOutcome::single_log(
        SetEnabledResponse {
            enabled: req.enabled,
            changed: true,
            mode: new_mode.as_str().to_string(),
        },
        format!(
            "memory_tree: set_enabled enabled={} mode={} changed=true",
            req.enabled,
            new_mode.as_str()
        ),
    ))
}
