//! Crash-reporting client ownership for the standalone terminal binary.

/// Initialize the Sentry client before the TUI installs its tracing subscriber.
///
/// The returned guard must live for the whole process. A build without the
/// `crash-reporting` feature retains the same call site and compiles to a no-op.
#[cfg(feature = "crash-reporting")]
pub fn init_crash_reporting() -> sentry::ClientInitGuard {
    // Match the core binary: startup consumers must see a repository-local
    // `.env`, while explicit process variables continue to take precedence.
    let _ = dotenvy::dotenv();

    sentry::init(sentry::ClientOptions {
        dsn: sentry_dsn(),
        release: Some(std::borrow::Cow::Owned(build_release_tag())),
        environment: Some(std::borrow::Cow::Owned(resolve_environment())),
        send_default_pii: false,
        before_send: Some(std::sync::Arc::new(|mut event| {
            event.server_name = None;
            for exception in &mut event.exception.values {
                if let Some(value) = exception.value.take() {
                    exception.value =
                        Some(openhuman_core::core::log_redaction::scrub_secrets(&value));
                }
            }
            if let Some(message) = event.message.take() {
                event.message = Some(openhuman_core::core::log_redaction::scrub_secrets(&message));
            }
            Some(event)
        })),
        sample_rate: 1.0,
        transport: Some(std::sync::Arc::new(
            openhuman_core::core::sentry_transport::factory,
        )),
        ..sentry::ClientOptions::default()
    })
}

#[cfg(not(feature = "crash-reporting"))]
pub fn init_crash_reporting() {}

#[cfg(feature = "crash-reporting")]
fn sentry_dsn() -> Option<sentry::types::Dsn> {
    std::env::var("OPENHUMAN_CORE_SENTRY_DSN")
        .ok()
        .filter(|value| !value.is_empty())
        .or_else(|| std::env::var("OPENHUMAN_SENTRY_DSN").ok())
        .filter(|value| !value.is_empty())
        .or_else(|| option_env!("OPENHUMAN_CORE_SENTRY_DSN").map(str::to_owned))
        .filter(|value| !value.is_empty())
        .or_else(|| option_env!("OPENHUMAN_SENTRY_DSN").map(str::to_owned))
        .filter(|value| !value.is_empty())
        .and_then(|value| value.parse().ok())
}

#[cfg(feature = "crash-reporting")]
fn build_release_tag() -> String {
    let version = env!("CARGO_PKG_VERSION");
    let short_sha: String = option_env!("OPENHUMAN_BUILD_SHA")
        .unwrap_or("")
        .trim()
        .chars()
        .take(12)
        .collect();
    if short_sha.is_empty() {
        format!("openhuman@{version}")
    } else {
        format!("openhuman@{version}+{short_sha}")
    }
}

#[cfg(feature = "crash-reporting")]
fn resolve_environment() -> String {
    if let Ok(value) = std::env::var("OPENHUMAN_APP_ENV") {
        let value = value.trim().to_ascii_lowercase();
        if !value.is_empty() {
            return value;
        }
    }
    if cfg!(debug_assertions) {
        "development".to_string()
    } else {
        "production".to_string()
    }
}

#[cfg(all(test, feature = "crash-reporting"))]
#[path = "crash_reporting_tests.rs"]
mod tests;
