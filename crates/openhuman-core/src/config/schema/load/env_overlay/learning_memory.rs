//! Env overrides for the learning subsystem and the memory tree.

use crate::config::schema::load::env::parse_env_bool;
use crate::config::schema::load::env::EnvLookup;
use crate::config::schema::Config;

impl Config {
    pub(super) fn apply_learning_env<E: EnvLookup + ?Sized>(&mut self, env: &E) {
        if let Some(flag) = env.get("OPENHUMAN_LEARNING_ENABLED") {
            let normalized = flag.trim().to_ascii_lowercase();
            match normalized.as_str() {
                "1" | "true" | "yes" | "on" => self.learning.enabled = true,
                "0" | "false" | "no" | "off" => self.learning.enabled = false,
                _ => {}
            }
        }
        if let Some(flag) = env.get("OPENHUMAN_LEARNING_REFLECTION_ENABLED") {
            let normalized = flag.trim().to_ascii_lowercase();
            match normalized.as_str() {
                "1" | "true" | "yes" | "on" => self.learning.reflection_enabled = true,
                "0" | "false" | "no" | "off" => self.learning.reflection_enabled = false,
                _ => {}
            }
        }
        if let Some(flag) = env.get("OPENHUMAN_LEARNING_USER_PROFILE_ENABLED") {
            let normalized = flag.trim().to_ascii_lowercase();
            match normalized.as_str() {
                "1" | "true" | "yes" | "on" => self.learning.user_profile_enabled = true,
                "0" | "false" | "no" | "off" => self.learning.user_profile_enabled = false,
                _ => {}
            }
        }
        if let Some(flag) = env.get("OPENHUMAN_LEARNING_TOOL_TRACKING_ENABLED") {
            let normalized = flag.trim().to_ascii_lowercase();
            match normalized.as_str() {
                "1" | "true" | "yes" | "on" => self.learning.tool_tracking_enabled = true,
                "0" | "false" | "no" | "off" => self.learning.tool_tracking_enabled = false,
                _ => {}
            }
        }
        if let Some(flag) = env.get("OPENHUMAN_LEARNING_TOOL_MEMORY_CAPTURE_ENABLED") {
            if let Some(enabled) = parse_env_bool(
                "OPENHUMAN_LEARNING_TOOL_MEMORY_CAPTURE_ENABLED",
                flag.as_str(),
            ) {
                self.learning.tool_memory_capture_enabled = enabled;
            }
        }
        if let Some(flag) = env.get("OPENHUMAN_LEARNING_EXPLICIT_PREFERENCES_ENABLED") {
            let normalized = flag.trim().to_ascii_lowercase();
            match normalized.as_str() {
                "1" | "true" | "yes" | "on" => self.learning.explicit_preferences_enabled = true,
                "0" | "false" | "no" | "off" => self.learning.explicit_preferences_enabled = false,
                _ => {}
            }
        }
        if let Some(flag) = env.get("OPENHUMAN_LEARNING_GOALS_ENRICHMENT_ENABLED") {
            let normalized = flag.trim().to_ascii_lowercase();
            match normalized.as_str() {
                "1" | "true" | "yes" | "on" => self.learning.goals_enrichment_enabled = true,
                "0" | "false" | "no" | "off" => self.learning.goals_enrichment_enabled = false,
                _ => {}
            }
        }
        if let Some(source) = env.get("OPENHUMAN_LEARNING_REFLECTION_SOURCE") {
            let normalized = source.trim().to_ascii_lowercase();
            match normalized.as_str() {
                "local" => self.learning.reflection_source = crate::config::ReflectionSource::Local,
                "cloud" => self.learning.reflection_source = crate::config::ReflectionSource::Cloud,
                _ => {
                    tracing::warn!(
                        source = %source,
                        "ignoring invalid OPENHUMAN_LEARNING_REFLECTION_SOURCE (valid: local, cloud)"
                    );
                }
            }
        }
        if let Some(val) = env.get("OPENHUMAN_LEARNING_MAX_REFLECTIONS_PER_SESSION") {
            if let Ok(max) = val.trim().parse::<usize>() {
                self.learning.max_reflections_per_session = max;
            }
        }
        if let Some(val) = env.get("OPENHUMAN_LEARNING_MIN_TURN_COMPLEXITY") {
            if let Ok(min) = val.trim().parse::<usize>() {
                self.learning.min_turn_complexity = min;
            }
        }
        if let Some(flag) = env.get("OPENHUMAN_LEARNING_EPISODIC_CAPTURE_ENABLED") {
            if let Some(enabled) =
                parse_env_bool("OPENHUMAN_LEARNING_EPISODIC_CAPTURE_ENABLED", flag.as_str())
            {
                self.learning.episodic_capture_enabled = enabled;
            }
        }
        if let Some(flag) = env.get("OPENHUMAN_LEARNING_STM_RECALL_ENABLED") {
            if let Some(enabled) =
                parse_env_bool("OPENHUMAN_LEARNING_STM_RECALL_ENABLED", flag.as_str())
            {
                self.learning.stm_recall_enabled = enabled;
            }
        }
    }

    pub(super) fn apply_memory_tree_env<E: EnvLookup + ?Sized>(&mut self, env: &E) {
        if let Ok(endpoint) = std::env::var("OPENHUMAN_MEMORY_EMBED_ENDPOINT") {
            let trimmed = endpoint.trim();
            self.memory_tree.embedding_endpoint = if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            };
        }
        if let Ok(model) = std::env::var("OPENHUMAN_MEMORY_EMBED_MODEL") {
            let trimmed = model.trim();
            self.memory_tree.embedding_model = if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            };
        }
        if let Ok(val) = std::env::var("OPENHUMAN_MEMORY_EMBED_TIMEOUT_MS") {
            if let Ok(timeout_ms) = val.trim().parse::<u64>() {
                if timeout_ms > 0 {
                    self.memory_tree.embedding_timeout_ms = Some(timeout_ms);
                }
            }
        }
        if let Ok(flag) = std::env::var("OPENHUMAN_MEMORY_EMBED_STRICT") {
            if let Some(strict) = parse_env_bool("OPENHUMAN_MEMORY_EMBED_STRICT", &flag) {
                self.memory_tree.embedding_strict = strict;
            }
        }
        if let Some(val) = env.get("OPENHUMAN_MEMORY_EMBED_RATE_LIMIT") {
            if let Ok(per_min) = val.trim().parse::<u32>() {
                self.memory.embedding_rate_limit_per_min = per_min;
            }
        }

        if let Ok(endpoint) = std::env::var("OPENHUMAN_MEMORY_EXTRACT_ENDPOINT") {
            let trimmed = endpoint.trim();
            self.memory_tree.llm_extractor_endpoint = if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            };
        }
        if let Ok(model) = std::env::var("OPENHUMAN_MEMORY_EXTRACT_MODEL") {
            let trimmed = model.trim();
            self.memory_tree.llm_extractor_model = if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            };
        }
        if let Ok(val) = std::env::var("OPENHUMAN_MEMORY_EXTRACT_TIMEOUT_MS") {
            if let Ok(ms) = val.trim().parse::<u64>() {
                if ms > 0 {
                    self.memory_tree.llm_extractor_timeout_ms = Some(ms);
                }
            }
        }

        if let Ok(endpoint) = std::env::var("OPENHUMAN_MEMORY_SUMMARISE_ENDPOINT") {
            let trimmed = endpoint.trim();
            self.memory_tree.llm_summariser_endpoint = if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            };
        }
        if let Ok(model) = std::env::var("OPENHUMAN_MEMORY_SUMMARISE_MODEL") {
            let trimmed = model.trim();
            self.memory_tree.llm_summariser_model = if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            };
        }
        if let Ok(val) = std::env::var("OPENHUMAN_MEMORY_SUMMARISE_TIMEOUT_MS") {
            if let Ok(ms) = val.trim().parse::<u64>() {
                if ms > 0 {
                    self.memory_tree.llm_summariser_timeout_ms = Some(ms);
                }
            }
        }

        if let Some(dir) = env.get("OPENHUMAN_MEMORY_TREE_CONTENT_DIR") {
            let trimmed = dir.trim();
            self.memory_tree.content_dir = if trimmed.is_empty() {
                None
            } else {
                Some(std::path::PathBuf::from(trimmed))
            };
        }

        if let Some(raw) = env.get("OPENHUMAN_MEMORY_TREE_LLM_BACKEND") {
            let trimmed = raw.trim();
            if !trimmed.is_empty() {
                match crate::config::LlmBackend::parse(trimmed) {
                    Ok(b) => {
                        log::debug!(
                            "[memory_tree] OPENHUMAN_MEMORY_TREE_LLM_BACKEND override applied: {}",
                            b.as_str()
                        );
                        self.memory_tree.llm_backend = b;
                    }
                    Err(e) => {
                        tracing::warn!(
                            value = trimmed,
                            error = %e,
                            "ignoring invalid OPENHUMAN_MEMORY_TREE_LLM_BACKEND (valid: cloud, local)"
                        );
                    }
                }
            }
        }
        if let Some(raw) = env.get("OPENHUMAN_MEMORY_TREE_CLOUD_LLM_MODEL") {
            let trimmed = raw.trim();
            self.memory_tree.cloud_llm_model = if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            };
        }

        if let Some(raw) = env.get("OPENHUMAN_MEMORY_TREE_SMART_WALK_MODEL") {
            let trimmed = raw.trim();
            self.memory_tree.smart_walk_model = if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            };
        }

        if let Some(raw) = env.get("OPENHUMAN_MEMORY_TREE_CLOUD_SUMMARIZATION") {
            if let Some(val) = parse_env_bool("OPENHUMAN_MEMORY_TREE_CLOUD_SUMMARIZATION", &raw) {
                self.memory_tree.cloud_summarization_opt_in = val;
            }
        }
        if let Some(raw) = env.get("OPENHUMAN_MEMORY_TREE_SPACY_ENABLED") {
            if let Some(val) = parse_env_bool("OPENHUMAN_MEMORY_TREE_SPACY_ENABLED", &raw) {
                self.memory_tree.spacy_enabled = val;
            }
        }
    }
}
