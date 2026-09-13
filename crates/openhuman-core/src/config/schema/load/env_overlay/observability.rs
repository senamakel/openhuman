//! Env overrides for Sentry, analytics, and agent-tracing content capture.

use crate::config::schema::load::env::EnvLookup;
use crate::config::schema::Config;

impl Config {
    pub(super) fn apply_observability_env<E: EnvLookup + ?Sized>(&mut self, env: &E) {
        let dsn_value = env
            .get("OPENHUMAN_CORE_SENTRY_DSN")
            .or_else(|| env.get("OPENHUMAN_SENTRY_DSN"))
            .or_else(|| option_env!("OPENHUMAN_CORE_SENTRY_DSN").map(|s| s.to_string()))
            .or_else(|| option_env!("OPENHUMAN_SENTRY_DSN").map(|s| s.to_string()));
        if let Some(dsn) = dsn_value {
            let dsn = dsn.trim();
            if !dsn.is_empty() {
                self.observability.sentry_dsn = Some(dsn.to_string());
            }
        }

        if let Some(flag) = env.get("OPENHUMAN_ANALYTICS_ENABLED") {
            let normalized = flag.trim().to_ascii_lowercase();
            match normalized.as_str() {
                "1" | "true" | "yes" | "on" => self.observability.analytics_enabled = true,
                "0" | "false" | "no" | "off" => self.observability.analytics_enabled = false,
                _ => {}
            }
        }

        // Opt-in: export prompt/reply content on trace spans (default off — a
        // deliberate PII reversal). Token/cost export is unaffected by this flag.
        if let Some(flag) = env.get("OPENHUMAN_AGENT_TRACING_CAPTURE_CONTENT") {
            let normalized = flag.trim().to_ascii_lowercase();
            match normalized.as_str() {
                "1" | "true" | "yes" | "on" => {
                    self.observability.agent_tracing.capture_content = true
                }
                "0" | "false" | "no" | "off" => {
                    self.observability.agent_tracing.capture_content = false
                }
                _ => {}
            }
        }
    }
}
