//! Base URL and defaults for the TinyHumans / AlphaHuman hosted API.

/// Default API host when `config.api_url` is unset or blank and no env override is set.
pub const DEFAULT_API_BASE_URL: &str = "https://api.tinyhumans.ai";
/// Default staging API host when the app environment is explicitly `staging`.
pub const DEFAULT_STAGING_API_BASE_URL: &str = "https://staging-api.tinyhumans.ai";
/// Primary app-environment selector used by the core and desktop app.
pub const APP_ENV_VAR: &str = "OPENHUMAN_APP_ENV";
/// Vite-exposed app-environment selector used by the frontend bundle.
pub const VITE_APP_ENV_VAR: &str = "VITE_OPENHUMAN_APP_ENV";

/// Resolves the hosted API base URL (no path suffix).
///
/// Order:
/// 1. Non-empty `api_url` from config (user explicitly set it)
/// 2. `BACKEND_URL` / `VITE_BACKEND_URL` runtime env vars (each checked independently)
/// 3. `BACKEND_URL` / `VITE_BACKEND_URL` baked in at compile time via `option_env!`
/// 4. Environment-aware default: `app_env_from_env()` == `staging` →
///    [`DEFAULT_STAGING_API_BASE_URL`], otherwise [`DEFAULT_API_BASE_URL`]
pub fn effective_api_url(api_url: &Option<String>) -> String {
    if let Some(u) = api_url.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        return normalize_api_base_url(u);
    }
    if let Some(env_url) = api_base_from_env() {
        return env_url;
    }
    default_api_base_url_for_env(app_env_from_env().as_deref()).to_string()
}

/// Trim and strip trailing slashes so paths join consistently.
pub fn normalize_api_base_url(url: &str) -> String {
    url.trim().trim_end_matches('/').to_string()
}

/// Resolve API base URL from the environment.
///
/// Each key is checked independently so that an empty `BACKEND_URL` does not
/// shadow a valid `VITE_BACKEND_URL`. Runtime vars are checked first, then
/// compile-time values baked in via `option_env!`. The compile-time path is
/// what makes a shipped DMG/installer resolve to the correct environment —
/// at runtime the process has no shell env vars set.
pub fn api_base_from_env() -> Option<String> {
    // 1. Runtime — each key checked independently; empty values are skipped
    //    so VITE_BACKEND_URL is still reachable when BACKEND_URL="" is set.
    for key in ["BACKEND_URL", "VITE_BACKEND_URL"] {
        if let Ok(v) = std::env::var(key) {
            let url = normalize_api_base_url(&v);
            if !url.is_empty() {
                return Some(url);
            }
        }
    }
    // 2. Compile-time fallback — baked in by build-desktop.yml.
    //    Each key checked independently for the same reason as above.
    for v in [option_env!("BACKEND_URL"), option_env!("VITE_BACKEND_URL")]
        .into_iter()
        .flatten()
    {
        let url = normalize_api_base_url(v);
        if !url.is_empty() {
            return Some(url);
        }
    }
    None
}

/// Resolve the app environment, checking runtime env first then compile-time.
///
/// Each key is checked independently so that an empty primary key does not
/// shadow a valid secondary key. The compile-time fallback (`option_env!`)
/// mirrors what the Tauri shell already does for its Sentry environment tag.
pub fn app_env_from_env() -> Option<String> {
    // 1. Runtime — each key checked independently
    for key in [APP_ENV_VAR, VITE_APP_ENV_VAR] {
        if let Ok(v) = std::env::var(key) {
            let s = v.trim().to_ascii_lowercase();
            if !s.is_empty() {
                return Some(s);
            }
        }
    }
    // 2. Compile-time fallback — each key checked independently
    for v in [
        option_env!("OPENHUMAN_APP_ENV"),
        option_env!("VITE_OPENHUMAN_APP_ENV"),
    ]
    .into_iter()
    .flatten()
    {
        let s = v.trim().to_ascii_lowercase();
        if !s.is_empty() {
            return Some(s);
        }
    }
    None
}

pub fn is_staging_app_env(app_env: Option<&str>) -> bool {
    matches!(app_env.map(str::trim), Some(env) if env.eq_ignore_ascii_case("staging"))
}

pub fn default_api_base_url_for_env(app_env: Option<&str>) -> &'static str {
    if is_staging_app_env(app_env) {
        DEFAULT_STAGING_API_BASE_URL
    } else {
        DEFAULT_API_BASE_URL
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Mutex, OnceLock};

    use super::*;

    // Serialise all env-mutating tests to prevent flaky failures under
    // parallel test execution (std::env is process-global).
    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    #[test]
    fn staging_app_env_uses_staging_default_api() {
        assert_eq!(
            default_api_base_url_for_env(Some("staging")),
            DEFAULT_STAGING_API_BASE_URL
        );
        assert!(is_staging_app_env(Some("STAGING")));
    }

    #[test]
    fn non_staging_app_env_uses_production_default_api() {
        assert_eq!(
            default_api_base_url_for_env(Some("production")),
            DEFAULT_API_BASE_URL
        );
        assert_eq!(default_api_base_url_for_env(None), DEFAULT_API_BASE_URL);
        assert!(!is_staging_app_env(Some("development")));
    }

    #[test]
    fn app_env_from_env_reads_runtime_var() {
        let _guard = ENV_LOCK.get_or_init(Mutex::default).lock().unwrap();
        let key = APP_ENV_VAR;
        let prev = std::env::var(key).ok();
        std::env::set_var(key, "staging");
        let result = app_env_from_env();
        match prev {
            Some(v) => std::env::set_var(key, v),
            None => std::env::remove_var(key),
        }
        assert_eq!(result.as_deref(), Some("staging"));
    }

    #[test]
    fn app_env_from_env_falls_through_empty_primary_to_secondary() {
        let _guard = ENV_LOCK.get_or_init(Mutex::default).lock().unwrap();
        let prev_primary = std::env::var(APP_ENV_VAR).ok();
        let prev_secondary = std::env::var(VITE_APP_ENV_VAR).ok();
        std::env::set_var(APP_ENV_VAR, ""); // empty — must not block secondary
        std::env::set_var(VITE_APP_ENV_VAR, "staging");
        let result = app_env_from_env();
        match prev_primary {
            Some(v) => std::env::set_var(APP_ENV_VAR, v),
            None => std::env::remove_var(APP_ENV_VAR),
        }
        match prev_secondary {
            Some(v) => std::env::set_var(VITE_APP_ENV_VAR, v),
            None => std::env::remove_var(VITE_APP_ENV_VAR),
        }
        assert_eq!(result.as_deref(), Some("staging"));
    }

    #[test]
    fn api_base_from_env_reads_runtime_var() {
        let _guard = ENV_LOCK.get_or_init(Mutex::default).lock().unwrap();
        let key = "BACKEND_URL";
        let prev = std::env::var(key).ok();
        std::env::set_var(key, "https://staging-api.tinyhumans.ai/");
        let result = api_base_from_env();
        match prev {
            Some(v) => std::env::set_var(key, v),
            None => std::env::remove_var(key),
        }
        assert_eq!(result.as_deref(), Some("https://staging-api.tinyhumans.ai"));
    }

    #[test]
    fn api_base_from_env_falls_through_empty_primary_to_secondary() {
        let _guard = ENV_LOCK.get_or_init(Mutex::default).lock().unwrap();
        let prev_primary = std::env::var("BACKEND_URL").ok();
        let prev_secondary = std::env::var("VITE_BACKEND_URL").ok();
        std::env::set_var("BACKEND_URL", ""); // empty — must not block secondary
        std::env::set_var("VITE_BACKEND_URL", "https://staging-api.tinyhumans.ai/");
        let result = api_base_from_env();
        match prev_primary {
            Some(v) => std::env::set_var("BACKEND_URL", v),
            None => std::env::remove_var("BACKEND_URL"),
        }
        match prev_secondary {
            Some(v) => std::env::set_var("VITE_BACKEND_URL", v),
            None => std::env::remove_var("VITE_BACKEND_URL"),
        }
        assert_eq!(result.as_deref(), Some("https://staging-api.tinyhumans.ai"));
    }
}
