use super::*;

#[test]
fn is_staging_app_env_matches_staging_case_insensitively() {
    assert!(is_staging_app_env(Some("staging")));
    assert!(is_staging_app_env(Some(" STAGING ")));
    assert!(!is_staging_app_env(Some("production")));
    assert!(!is_staging_app_env(None));
}

#[test]
fn app_env_from_env_reads_runtime_var() {
    // Setting APP_ENV to "staging" flips `default_root_dir_name()` to
    // `.openhuman-staging` process-wide, which breaks any concurrent test
    // resolving the root openhuman dir. Hold the crate-wide env lock too,
    // in the established order (TEST_ENV_LOCK before the backend lock).
    let _env_guard = crate::config::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let _guard = env_test_lock();
    let prev = std::env::var(APP_ENV_VAR).ok();
    std::env::set_var(APP_ENV_VAR, "staging");
    let result = app_env_from_env();
    match prev {
        Some(v) => std::env::set_var(APP_ENV_VAR, v),
        None => std::env::remove_var(APP_ENV_VAR),
    }
    assert_eq!(result.as_deref(), Some("staging"));
}

#[test]
fn app_env_empty_primary_falls_through_to_secondary() {
    // Same staging-root hazard as `app_env_from_env_reads_runtime_var`.
    let _env_guard = crate::config::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let _guard = env_test_lock();
    let prev_p = std::env::var(APP_ENV_VAR).ok();
    let prev_s = std::env::var(VITE_APP_ENV_VAR).ok();
    std::env::set_var(APP_ENV_VAR, "");
    std::env::set_var(VITE_APP_ENV_VAR, "staging");
    let result = app_env_from_env();
    match prev_p {
        Some(v) => std::env::set_var(APP_ENV_VAR, v),
        None => std::env::remove_var(APP_ENV_VAR),
    }
    match prev_s {
        Some(v) => std::env::set_var(VITE_APP_ENV_VAR, v),
        None => std::env::remove_var(VITE_APP_ENV_VAR),
    }
    assert_eq!(result.as_deref(), Some("staging"));
}
