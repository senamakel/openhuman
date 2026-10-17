//! Shared website, origin, and URL safety policy for browser calls.

use super::BrowserCallError;

pub(super) fn parsed_browser_url(raw: &str) -> Result<url::Url, BrowserCallError> {
    url::Url::parse(raw)
        .or_else(|_| url::Url::parse(&format!("https://{raw}")))
        .map_err(|_| BrowserCallError::Policy("invalid URL".into()))
}

pub(super) fn browser_allow_all() -> bool {
    matches!(
        std::env::var("OPENHUMAN_BROWSER_ALLOW_ALL").ok().as_deref(),
        Some("1" | "true" | "TRUE" | "yes" | "YES")
    )
}

pub(super) fn private_or_local(host: &str) -> bool {
    // Preserve browser policy for multiple trailing dots before using the
    // canonical shared classifier. Browser URL parsing can retain these.
    tinytools_std::url_guard::is_private_or_local_host(
        &host.trim_end_matches('.').to_ascii_lowercase(),
    )
}
