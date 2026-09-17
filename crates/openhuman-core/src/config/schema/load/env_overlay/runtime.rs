//! Env overrides for the local-AI tier preset and the Node, Python, runtime-pool, and tokenjuice runtimes.

use crate::config::schema::load::env::parse_env_bool;
use crate::config::schema::load::env::EnvLookup;
use crate::config::schema::Config;

impl Config {
    pub(super) fn apply_runtime_env<E: EnvLookup + ?Sized>(&mut self, env: &E) {
        if let Some(tier_str) = env.get("OPENHUMAN_LOCAL_AI_TIER") {
            let tier_str = tier_str.trim().to_ascii_lowercase();
            if !tier_str.is_empty() {
                if let Some(tier) = crate::inference::presets::ModelTier::from_str_opt(&tier_str) {
                    if tier == crate::inference::presets::ModelTier::Custom {
                        tracing::warn!(
                            tier = %tier_str,
                            "ignoring custom OPENHUMAN_LOCAL_AI_TIER; only built-in presets are supported"
                        );
                    } else if !tier.is_mvp_allowed() {
                        tracing::warn!(
                            tier = %tier_str,
                            "ignoring OPENHUMAN_LOCAL_AI_TIER outside the 1B local-model allowlist"
                        );
                    } else {
                        crate::inference::presets::apply_preset_to_config(&mut self.local_ai, tier);
                        tracing::debug!(
                            tier = %tier_str,
                            "applied local AI tier from OPENHUMAN_LOCAL_AI_TIER"
                        );
                    }
                } else {
                    tracing::warn!(
                        tier = %tier_str,
                        "ignoring invalid OPENHUMAN_LOCAL_AI_TIER (valid: ram_2_4gb)"
                    );
                }
            }
        }

        if let Some(flag) = env.get("OPENHUMAN_NODE_ENABLED") {
            if let Some(enabled) = parse_env_bool("OPENHUMAN_NODE_ENABLED", &flag) {
                self.node.enabled = enabled;
            }
        }
        if let Some(version) = env.get("OPENHUMAN_NODE_VERSION") {
            let trimmed = version.trim();
            if !trimmed.is_empty() {
                self.node.version = trimmed.to_string();
            }
        }
        if let Some(dir) = env.get("OPENHUMAN_NODE_CACHE_DIR") {
            let trimmed = dir.trim();
            if !trimmed.is_empty() {
                self.node.cache_dir = trimmed.to_string();
            }
        }
        if let Some(flag) = env.get("OPENHUMAN_NODE_PREFER_SYSTEM") {
            if let Some(prefer_system) = parse_env_bool("OPENHUMAN_NODE_PREFER_SYSTEM", &flag) {
                self.node.prefer_system = prefer_system;
            }
        }

        if let Some(flag) = env.get("OPENHUMAN_RUNTIME_PYTHON_ENABLED") {
            if let Some(enabled) = parse_env_bool("OPENHUMAN_RUNTIME_PYTHON_ENABLED", &flag) {
                self.runtime_python.enabled = enabled;
            }
        }
        if let Some(version) = env.get("OPENHUMAN_RUNTIME_PYTHON_MINIMUM_VERSION") {
            let trimmed = version.trim();
            if !trimmed.is_empty() {
                self.runtime_python.minimum_version = trimmed.to_string();
            }
        }
        if let Some(dir) = env.get("OPENHUMAN_RUNTIME_PYTHON_CACHE_DIR") {
            self.runtime_python.cache_dir = dir.trim().to_string();
        }
        if let Some(tag) = env.get("OPENHUMAN_RUNTIME_PYTHON_MANAGED_RELEASE_TAG") {
            self.runtime_python.managed_release_tag = tag.trim().to_string();
        }
        if let Some(flag) = env.get("OPENHUMAN_RUNTIME_PYTHON_PREFER_SYSTEM") {
            if let Some(prefer_system) =
                parse_env_bool("OPENHUMAN_RUNTIME_PYTHON_PREFER_SYSTEM", &flag)
            {
                self.runtime_python.prefer_system = prefer_system;
            }
        }
        if let Some(command) = env.get("OPENHUMAN_RUNTIME_PYTHON_PREFERRED_COMMAND") {
            self.runtime_python.preferred_command = command.trim().to_string();
        }

        // --- Shared language-runtime pool (#5106) --------------------------
        if let Some(flag) = env.get("OPENHUMAN_RUNTIME_POOL_ENABLED") {
            if let Some(enabled) = parse_env_bool("OPENHUMAN_RUNTIME_POOL_ENABLED", &flag) {
                self.runtime_pool.enabled = enabled;
            }
        }
        if let Some(raw) = env.get("OPENHUMAN_RUNTIME_POOL_NODE_MAX_WORKERS") {
            match raw.trim().parse::<usize>() {
                Ok(n) => self.runtime_pool.node.max_workers = n,
                Err(e) => tracing::warn!(
                    value = %raw,
                    error = %e,
                    "[config] ignoring invalid OPENHUMAN_RUNTIME_POOL_NODE_MAX_WORKERS"
                ),
            }
        }
        if let Some(raw) = env.get("OPENHUMAN_RUNTIME_POOL_PYTHON_MAX_WORKERS") {
            match raw.trim().parse::<usize>() {
                Ok(n) => self.runtime_pool.python.max_workers = n,
                Err(e) => tracing::warn!(
                    value = %raw,
                    error = %e,
                    "[config] ignoring invalid OPENHUMAN_RUNTIME_POOL_PYTHON_MAX_WORKERS"
                ),
            }
        }

        // --- TokenJuice content router -------------------------------------
        if let Some(flag) = env.get("OPENHUMAN_TOKENJUICE_ENABLED") {
            if let Some(v) = parse_env_bool("OPENHUMAN_TOKENJUICE_ENABLED", &flag) {
                self.tokenjuice.router_enabled = v;
            }
        }
        if let Some(flag) = env.get("OPENHUMAN_TOKENJUICE_CCR_ENABLED") {
            if let Some(v) = parse_env_bool("OPENHUMAN_TOKENJUICE_CCR_ENABLED", &flag) {
                self.tokenjuice.ccr_enabled = v;
            }
        }
        if let Some(flag) = env.get("OPENHUMAN_TOKENJUICE_CCR_DISK_ENABLED") {
            if let Some(v) = parse_env_bool("OPENHUMAN_TOKENJUICE_CCR_DISK_ENABLED", &flag) {
                self.tokenjuice.ccr_disk_enabled = v;
            }
        }
        if let Some(flag) = env.get("OPENHUMAN_TOKENJUICE_SEARCH_ENABLED") {
            if let Some(v) = parse_env_bool("OPENHUMAN_TOKENJUICE_SEARCH_ENABLED", &flag) {
                self.tokenjuice.search_enabled = v;
            }
        }
        if let Some(flag) = env.get("OPENHUMAN_TOKENJUICE_CODE_ENABLED") {
            if let Some(v) = parse_env_bool("OPENHUMAN_TOKENJUICE_CODE_ENABLED", &flag) {
                self.tokenjuice.code_enabled = v;
            }
        }
        if let Some(flag) = env.get("OPENHUMAN_TOKENJUICE_HTML_ENABLED") {
            if let Some(v) = parse_env_bool("OPENHUMAN_TOKENJUICE_HTML_ENABLED", &flag) {
                self.tokenjuice.html_enabled = v;
            }
        }
        if let Some(s) = env.get("OPENHUMAN_TOKENJUICE_MAX_CACHE_ENTRIES") {
            if let Ok(v) = s.trim().parse::<usize>() {
                self.tokenjuice.max_cache_entries = v;
            }
        }
        if let Some(s) = env.get("OPENHUMAN_TOKENJUICE_MAX_CACHE_BYTES") {
            if let Ok(v) = s.trim().parse::<usize>() {
                self.tokenjuice.max_cache_bytes = v;
            }
        }
        if let Some(s) = env.get("OPENHUMAN_TOKENJUICE_CCR_TTL_SECS") {
            if let Ok(v) = s.trim().parse::<u64>() {
                self.tokenjuice.ccr_ttl_secs = Some(v);
            }
        }
        if let Some(s) = env.get("OPENHUMAN_TOKENJUICE_CCR_MIN_TOKENS") {
            if let Ok(v) = s.trim().parse::<usize>() {
                self.tokenjuice.ccr_min_tokens = v;
            }
        }
        // ML plain-text compressor (Kompress).
        if let Some(flag) = env.get("OPENHUMAN_TOKENJUICE_ML_COMPRESSION_ENABLED") {
            if let Some(v) = parse_env_bool("OPENHUMAN_TOKENJUICE_ML_COMPRESSION_ENABLED", &flag) {
                self.tokenjuice.ml_compression_enabled = v;
            }
        }
        if let Some(m) = env.get("OPENHUMAN_TOKENJUICE_ML_MODEL_ID") {
            let t = m.trim();
            if !t.is_empty() {
                self.tokenjuice.ml_model_id = t.to_string();
            }
        }
        if let Some(d) = env.get("OPENHUMAN_TOKENJUICE_ML_DEVICE") {
            let t = d.trim();
            if !t.is_empty() {
                self.tokenjuice.ml_device = t.to_string();
            }
        }
        if let Some(r) = env.get("OPENHUMAN_TOKENJUICE_ML_TARGET_RATIO") {
            if let Ok(v) = r.trim().parse::<f64>() {
                if (0.0..=1.0).contains(&v) {
                    self.tokenjuice.ml_target_ratio = v;
                }
            }
        }
        if let Some(s) = env.get("OPENHUMAN_TOKENJUICE_ML_SIDECAR_IDLE_TIMEOUT_SECS") {
            if let Ok(v) = s.trim().parse::<u64>() {
                self.tokenjuice.ml_sidecar_idle_timeout_secs = v;
            }
        }
        if let Some(s) = env.get("OPENHUMAN_TOKENJUICE_ML_MAX_INPUT_CHARS") {
            if let Ok(v) = s.trim().parse::<usize>() {
                self.tokenjuice.ml_max_input_chars = v;
            }
        }
    }
}
