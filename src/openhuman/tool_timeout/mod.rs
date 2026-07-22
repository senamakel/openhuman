//! OpenHuman configuration shim for TinyAgents tool deadlines.
//!
//! TinyAgents owns timeout vocabulary, dynamic storage, resolution, grace, and
//! enforcement. This module keeps only product configuration precedence and
//! the seconds-based helpers used by scripting tools.

use std::sync::LazyLock;
use std::time::Duration;

use tinyagents::harness::tool::ToolTimeoutSettings;

pub const DEFAULT_TIMEOUT_SECS: u64 = 120;
pub const MIN_TIMEOUT_SECS: u64 = 1;
pub const MAX_TIMEOUT_SECS: u64 = 3_600;
pub const SANDBOX_UNBOUNDED_CAP_SECS: u64 = 86_400;
pub const ENV_VAR: &str = "OPENHUMAN_TOOL_TIMEOUT_SECS";

const TOOL_TIMEOUT_GRACE_SECS: u64 = 5;

static SETTINGS: LazyLock<ToolTimeoutSettings> = LazyLock::new(|| {
    ToolTimeoutSettings::new(
        DEFAULT_TIMEOUT_SECS * 1_000,
        MIN_TIMEOUT_SECS * 1_000,
        MAX_TIMEOUT_SECS * 1_000,
        TOOL_TIMEOUT_GRACE_SECS * 1_000,
    )
});

/// Shared settings installed on every OpenHuman TinyAgents run policy.
pub fn settings() -> ToolTimeoutSettings {
    SETTINGS.clone()
}

/// Parses a bounded seconds value, falling back to the product default.
pub fn parse_tool_timeout_secs(raw: Option<&str>) -> u64 {
    bounded_secs(raw).unwrap_or(DEFAULT_TIMEOUT_SECS)
}

fn bounded_secs(raw: Option<&str>) -> Option<u64> {
    raw.and_then(|value| value.parse::<u64>().ok())
        .filter(|value| (MIN_TIMEOUT_SECS..=MAX_TIMEOUT_SECS).contains(value))
}

fn read_env() -> Option<String> {
    std::env::var(ENV_VAR).ok()
}

/// Whether a valid operator override currently wins over persisted config.
pub fn env_override_active() -> bool {
    bounded_secs(read_env().as_deref()).is_some()
}

/// Pushes persisted config into the crate-owned runtime settings.
///
/// A valid operator env override wins. Invalid config values fall back to the
/// product default. Returns the effective seconds value.
pub fn set_tool_timeout_secs(config_secs: u64) -> u64 {
    let env = read_env();
    let effective = bounded_secs(env.as_deref())
        .unwrap_or_else(|| parse_tool_timeout_secs(Some(&config_secs.to_string())));
    SETTINGS.set_inherited_timeout_ms(effective.saturating_mul(1_000));
    if bounded_secs(env.as_deref()).is_some() {
        log::debug!(
            "[tool_timeout] config update ignored: env {ENV_VAR}={effective}s overrides requested {config_secs}s"
        );
    } else {
        log::debug!(
            "[tool_timeout] runtime timeout set to {effective}s (requested {config_secs}s)"
        );
    }
    effective
}

/// Effective inherited timeout in seconds, read fresh from TinyAgents.
pub fn tool_execution_timeout_secs() -> u64 {
    SETTINGS
        .inherited_timeout()
        .unwrap_or(Duration::ZERO)
        .as_secs()
}

/// Resolves an optional seconds budget for a scripting tool.
///
/// Missing or zero means unbounded. Other values clamp to the product range
/// and the caller's tighter cap.
pub fn explicit_call_timeout_secs(requested: Option<u64>, cap: u64) -> Option<u64> {
    let cap = cap.clamp(MIN_TIMEOUT_SECS, MAX_TIMEOUT_SECS);
    match requested {
        None | Some(0) => None,
        Some(value) => Some(value.clamp(MIN_TIMEOUT_SECS, cap)),
    }
}

pub fn explicit_call_timeout_duration(requested: Option<u64>, cap: u64) -> Option<Duration> {
    explicit_call_timeout_secs(requested, cap).map(Duration::from_secs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_accepts_only_product_bounds() {
        assert_eq!(parse_tool_timeout_secs(None), DEFAULT_TIMEOUT_SECS);
        assert_eq!(parse_tool_timeout_secs(Some("nope")), DEFAULT_TIMEOUT_SECS);
        assert_eq!(parse_tool_timeout_secs(Some("0")), DEFAULT_TIMEOUT_SECS);
        assert_eq!(parse_tool_timeout_secs(Some("3601")), DEFAULT_TIMEOUT_SECS);
        assert_eq!(parse_tool_timeout_secs(Some("1")), MIN_TIMEOUT_SECS);
        assert_eq!(parse_tool_timeout_secs(Some("3600")), MAX_TIMEOUT_SECS);
        assert_eq!(parse_tool_timeout_secs(Some("300")), 300);
    }

    #[test]
    fn explicit_budget_preserves_unbounded_and_clamps() {
        assert_eq!(explicit_call_timeout_secs(None, MAX_TIMEOUT_SECS), None);
        assert_eq!(explicit_call_timeout_secs(Some(0), MAX_TIMEOUT_SECS), None);
        assert_eq!(explicit_call_timeout_secs(Some(600), 1_800), Some(600));
        assert_eq!(explicit_call_timeout_secs(Some(99_999), 1_800), Some(1_800));
        assert_eq!(
            explicit_call_timeout_duration(Some(2), MAX_TIMEOUT_SECS),
            Some(Duration::from_secs(2))
        );
    }
}
