//! Env overrides for dictation and the context-management budget.

use crate::config::schema::load::env::EnvLookup;
use crate::config::schema::Config;

impl Config {
    pub(super) fn apply_dictation_env<E: EnvLookup + ?Sized>(&mut self, env: &E) {
        if let Some(flag) = env.get("OPENHUMAN_DICTATION_ENABLED") {
            let normalized = flag.trim().to_ascii_lowercase();
            match normalized.as_str() {
                "1" | "true" | "yes" | "on" => self.dictation.enabled = true,
                "0" | "false" | "no" | "off" => self.dictation.enabled = false,
                _ => {}
            }
        }
        if let Some(hotkey) = env.get("OPENHUMAN_DICTATION_HOTKEY") {
            let hotkey = hotkey.trim();
            if !hotkey.is_empty() {
                self.dictation.hotkey = hotkey.to_string();
            }
        }
        if let Some(mode) = env.get("OPENHUMAN_DICTATION_ACTIVATION_MODE") {
            let normalized = mode.trim().to_ascii_lowercase();
            match normalized.as_str() {
                "toggle" => {
                    self.dictation.activation_mode = crate::config::DictationActivationMode::Toggle
                }
                "push" => {
                    self.dictation.activation_mode = crate::config::DictationActivationMode::Push
                }
                _ => {
                    tracing::warn!(
                        mode = %mode,
                        "ignoring invalid OPENHUMAN_DICTATION_ACTIVATION_MODE (valid: toggle, push)"
                    );
                }
            }
        }
        if let Some(flag) = env.get("OPENHUMAN_DICTATION_LLM_REFINEMENT") {
            let normalized = flag.trim().to_ascii_lowercase();
            match normalized.as_str() {
                "1" | "true" | "yes" | "on" => self.dictation.llm_refinement = true,
                "0" | "false" | "no" | "off" => self.dictation.llm_refinement = false,
                _ => {}
            }
        }
        if let Some(flag) = env.get("OPENHUMAN_DICTATION_STREAMING") {
            let normalized = flag.trim().to_ascii_lowercase();
            match normalized.as_str() {
                "1" | "true" | "yes" | "on" => self.dictation.streaming = true,
                "0" | "false" | "no" | "off" => self.dictation.streaming = false,
                _ => {}
            }
        }
        if let Some(val) = env.get("OPENHUMAN_DICTATION_STREAMING_INTERVAL_MS") {
            if let Ok(ms) = val.trim().parse::<u64>() {
                self.dictation.streaming_interval_ms = ms;
            }
        }
    }

    pub(super) fn apply_context_env<E: EnvLookup + ?Sized>(&mut self, env: &E) {
        if let Some(flag) = env.get("OPENHUMAN_CONTEXT_ENABLED") {
            let normalized = flag.trim().to_ascii_lowercase();
            match normalized.as_str() {
                "1" | "true" | "yes" | "on" => self.context.enabled = true,
                "0" | "false" | "no" | "off" => self.context.enabled = false,
                _ => {}
            }
        }
        if let Some(flag) = env.get("OPENHUMAN_CONTEXT_MICROCOMPACT_ENABLED") {
            let normalized = flag.trim().to_ascii_lowercase();
            match normalized.as_str() {
                "1" | "true" | "yes" | "on" => self.context.microcompact_enabled = true,
                "0" | "false" | "no" | "off" => self.context.microcompact_enabled = false,
                _ => {}
            }
        }
        if let Some(flag) = env.get("OPENHUMAN_CONTEXT_AUTOCOMPACT_ENABLED") {
            let normalized = flag.trim().to_ascii_lowercase();
            match normalized.as_str() {
                "1" | "true" | "yes" | "on" => self.context.autocompact_enabled = true,
                "0" | "false" | "no" | "off" => self.context.autocompact_enabled = false,
                _ => {}
            }
        }
        if let Some(val) = env.get("OPENHUMAN_CONTEXT_TOOL_RESULT_BUDGET_BYTES") {
            if let Ok(n) = val.trim().parse::<usize>() {
                self.context.tool_result_budget_bytes = n;
            }
        }
        // Kill-switch for native tool-output compaction (Stage 1a). On by
        // default; `OPENHUMAN_COMPACTION=0` disables it for a support/A-B
        // bisect. Accepts the canonical short name and the namespaced form.
        if let Some(flag) = env
            .get("OPENHUMAN_COMPACTION")
            .or_else(|| env.get("OPENHUMAN_CONTEXT_COMPACTION_ENABLED"))
        {
            match flag.trim().to_ascii_lowercase().as_str() {
                "1" | "true" | "yes" | "on" => self.context.compaction_enabled = true,
                "0" | "false" | "no" | "off" => self.context.compaction_enabled = false,
                _ => {}
            }
        }
        if let Some(model) = env.get("OPENHUMAN_CONTEXT_SUMMARIZER_MODEL") {
            let model = model.trim();
            if !model.is_empty() {
                self.context.summarizer_model = Some(model.to_string());
            }
        }
        let context_default = crate::agent::context::DEFAULT_TOOL_RESULT_BUDGET_BYTES;
        let context_env_set = env.contains("OPENHUMAN_CONTEXT_TOOL_RESULT_BUDGET_BYTES");
        if !context_env_set
            && self.context.tool_result_budget_bytes == context_default
            && self.agent.tool_result_budget_bytes != context_default
        {
            tracing::warn!(
                old = self.agent.tool_result_budget_bytes,
                "[context:config] `agent.tool_result_budget_bytes` is \
                 deprecated — please move it to \
                 `context.tool_result_budget_bytes` in your config.toml"
            );
            self.context.tool_result_budget_bytes = self.agent.tool_result_budget_bytes;
        }
    }
}
