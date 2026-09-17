//! Env overrides for the `[subsystems]` drivers and the auto-updater.

use crate::config::schema::load::env::parse_env_bool;
use crate::config::schema::load::env::EnvLookup;
use crate::config::schema::Config;
use crate::config::schema::UpdateRestartStrategy;

impl Config {
    /// `[subsystems.memory]` overrides — kernel.md §3.6 / plan-memory.md §4.5. Mirrors
    /// the `apply_memory_tree_env` reading pattern above. GREENFIELD: nothing
    /// reads `self.subsystems` yet, so these overrides have no runtime effect
    /// beyond making the field settable via env for forward compatibility.
    pub(super) fn apply_subsystems_env<E: EnvLookup + ?Sized>(&mut self, env: &E) {
        if let Some(raw) = env.get("OPENHUMAN_MEMORY_DRIVER") {
            let trimmed = raw.trim();
            if !trimmed.is_empty() {
                self.subsystems.memory.driver = trimmed.to_string();
            }
        }

        if let Some(raw) = env.get("OPENHUMAN_MEMORY_HOOKS_AUTO_RECALL") {
            if let Some(val) = parse_env_bool("OPENHUMAN_MEMORY_HOOKS_AUTO_RECALL", &raw) {
                self.subsystems.memory.hooks.auto_recall = val;
            }
        }
        if let Some(raw) = env.get("OPENHUMAN_MEMORY_HOOKS_AUTO_CAPTURE") {
            if let Some(val) = parse_env_bool("OPENHUMAN_MEMORY_HOOKS_AUTO_CAPTURE", &raw) {
                self.subsystems.memory.hooks.auto_capture = val;
            }
        }
        if let Some(raw) = env.get("OPENHUMAN_MEMORY_HOOKS_MAX_CONTEXT_TOKENS") {
            let trimmed = raw.trim();
            if !trimmed.is_empty() {
                match trimmed.parse::<usize>() {
                    Ok(v) => self.subsystems.memory.hooks.max_context_tokens = v,
                    Err(_) => tracing::warn!(
                        value = %raw,
                        "invalid OPENHUMAN_MEMORY_HOOKS_MAX_CONTEXT_TOKENS ignored; expected an unsigned integer"
                    ),
                }
            }
        }
        if let Some(raw) = env.get("OPENHUMAN_MEMORY_HOOKS_RECALL_MAX_CHARS") {
            let trimmed = raw.trim();
            if !trimmed.is_empty() {
                match trimmed.parse::<usize>() {
                    Ok(v) => self.subsystems.memory.hooks.recall_max_chars = v,
                    Err(_) => tracing::warn!(
                        value = %raw,
                        "invalid OPENHUMAN_MEMORY_HOOKS_RECALL_MAX_CHARS ignored; expected an unsigned integer"
                    ),
                }
            }
        }
        if let Some(raw) = env.get("OPENHUMAN_MEMORY_HOOKS_CAPTURE_MAX_CHARS") {
            let trimmed = raw.trim();
            if !trimmed.is_empty() {
                match trimmed.parse::<usize>() {
                    Ok(v) => self.subsystems.memory.hooks.capture_max_chars = v,
                    Err(_) => tracing::warn!(
                        value = %raw,
                        "invalid OPENHUMAN_MEMORY_HOOKS_CAPTURE_MAX_CHARS ignored; expected an unsigned integer"
                    ),
                }
            }
        }
    }

    pub(super) fn apply_update_env<E: EnvLookup + ?Sized>(&mut self, env: &E) {
        if let Some(flag) = env.get("OPENHUMAN_AUTO_UPDATE_ENABLED") {
            let normalized = flag.trim().to_ascii_lowercase();
            match normalized.as_str() {
                "1" | "true" | "yes" | "on" => self.update.enabled = true,
                "0" | "false" | "no" | "off" => self.update.enabled = false,
                _ => {}
            }
        }
        if let Some(val) = env.get("OPENHUMAN_AUTO_UPDATE_INTERVAL_MINUTES") {
            if let Ok(minutes) = val.trim().parse::<u32>() {
                self.update.interval_minutes = minutes;
            }
        }
        if let Some(raw) = env.get("OPENHUMAN_AUTO_UPDATE_RESTART_STRATEGY") {
            match raw.trim().to_ascii_lowercase().as_str() {
                "self_replace" | "self-replace" | "self" => {
                    self.update.restart_strategy = UpdateRestartStrategy::SelfReplace;
                }
                "supervisor" | "stage_only" | "stage-only" => {
                    self.update.restart_strategy = UpdateRestartStrategy::Supervisor;
                }
                other => {
                    tracing::warn!(
                        value = other,
                        "ignoring invalid OPENHUMAN_AUTO_UPDATE_RESTART_STRATEGY \
                         (valid: self_replace, supervisor)"
                    );
                }
            }
        }
        if let Some(flag) = env.get("OPENHUMAN_AUTO_UPDATE_RPC_MUTATIONS_ENABLED") {
            if let Some(enabled) =
                parse_env_bool("OPENHUMAN_AUTO_UPDATE_RPC_MUTATIONS_ENABLED", &flag)
            {
                self.update.rpc_mutations_enabled = enabled;
            }
        }
    }
}
