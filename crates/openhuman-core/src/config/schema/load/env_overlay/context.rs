//! Env overrides for the context-management budget.

use crate::config::schema::load::env::EnvLookup;
use crate::config::schema::Config;

impl Config {
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
        if let Some(val) = env.get("OPENHUMAN_COMPACTION_STRATEGY") {
            match crate::config::CompactionStrategy::parse(&val) {
                Some(strategy) => self.context.compaction_strategy = strategy,
                None if val.trim().is_empty() => {}
                None => tracing::warn!(
                    value = %val.trim(),
                    "[config] ignoring unknown OPENHUMAN_COMPACTION_STRATEGY (task_state | summary)"
                ),
            }
        }
        // Absolute compaction trigger (benchmarks / debugging). `0` or an
        // empty value clears an override set in `config.toml`.
        if let Some(val) = env.get("OPENHUMAN_COMPACTION_TRIGGER_TOKENS") {
            match val.trim() {
                "" | "0" => self.context.compaction_trigger_tokens = None,
                raw => match raw.parse::<u64>() {
                    Ok(tokens) => self.context.compaction_trigger_tokens = Some(tokens),
                    Err(_) => tracing::warn!(
                        value = raw,
                        "[config] ignoring unparseable OPENHUMAN_COMPACTION_TRIGGER_TOKENS"
                    ),
                },
            }
        }
        if let Some(val) = env.get("OPENHUMAN_CONTEXT_TOOL_RESULT_BUDGET_BYTES") {
            if let Ok(n) = val.trim().parse::<usize>() {
                self.context.tool_result_budget_bytes = n;
            }
        }
        // Switch for native tool-output compaction (Stage 1a). On by
        // default; `OPENHUMAN_COMPACTION=0` opts out (and is the A/B bisect).
        // Accepts the canonical short name and the namespaced form.
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
        let context_default =
            tinyagents_harness::config::TurnConfig::default().tool_result_budget_bytes;
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
