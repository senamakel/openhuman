//! LinkedIn profile enrichment via Gmail email mining + Apify scraping.
//!
//! Pipeline:
//!
//! 1. Search Gmail (via Composio) for emails from `linkedin.com`.
//! 2. Extract a `linkedin.com/in/<slug>` profile URL from the results.
//! 3. Scrape the profile via the Apify actor `dev_fusion/linkedin-profile-scraper`.
//! 4. Persist the scraped profile data into the user-profile memory namespace.
//!
//! Designed to run once during onboarding as a fire-and-forget enrichment
//! pass. Each stage logs progress so the caller (or a future frontend
//! progress UI) can observe what happened.

use crate::openhuman::config::Config;
use crate::openhuman::integrations::{build_client, IntegrationClient};
use regex::Regex;
use serde_json::json;
use std::sync::{Arc, LazyLock};

/// Apify actor slug for the LinkedIn profile scraper.
const LINKEDIN_SCRAPER_ACTOR: &str = "dev_fusion/linkedin-profile-scraper";

/// Regex that captures a LinkedIn username from profile URLs.
///
/// Matches both the canonical form (`linkedin.com/in/<slug>`) and the
/// notification-email form (`linkedin.com/comm/in/<slug>`). The username
/// is captured in group 1 so we can reconstruct a clean canonical URL.
static LINKEDIN_USERNAME_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"https?://(?:www\.)?linkedin\.com/(?:comm/)?in/([a-zA-Z0-9_-]+)").unwrap()
});

/// Build the canonical profile URL from a username slug.
fn canonical_linkedin_url(username: &str) -> String {
    format!("https://www.linkedin.com/in/{username}")
}

/// Outcome of the full enrichment pipeline.
#[derive(Debug)]
pub struct LinkedInEnrichmentResult {
    /// The LinkedIn profile URL found in Gmail, if any.
    pub profile_url: Option<String>,
    /// Raw scraped profile JSON from Apify, if the scrape succeeded.
    pub profile_data: Option<serde_json::Value>,
    /// Human-readable summary of what happened at each stage.
    pub log: Vec<String>,
}

/// Run the full Gmail → LinkedIn �� Apify enrichment pipeline.
///
/// Returns `Ok` with a result struct even if individual stages fail —
/// partial progress is still useful. Only returns `Err` if we can't
/// even build the integration client (i.e. user isn't signed in).
pub async fn run_linkedin_enrichment(config: &Config) -> anyhow::Result<LinkedInEnrichmentResult> {
    let client = build_client(config)
        .ok_or_else(|| anyhow::anyhow!("no integration client — user not signed in"))?;

    let mut result = LinkedInEnrichmentResult {
        profile_url: None,
        profile_data: None,
        log: Vec::new(),
    };

    // ── Stage 1: search Gmail for LinkedIn emails ��───────────────────
    tracing::info!("[linkedin_enrichment] stage 1: searching Gmail for LinkedIn emails");
    result.log.push("Searching Gmail for LinkedIn emails...".into());

    let profile_url = match search_gmail_for_linkedin(config).await {
        Ok(Some(url)) => {
            tracing::info!(url = %url, "[linkedin_enrichment] found LinkedIn profile URL");
            result.log.push(format!("Found LinkedIn profile: {url}"));
            Some(url)
        }
        Ok(None) => {
            tracing::info!("[linkedin_enrichment] no LinkedIn profile URL found in emails");
            result.log.push("No LinkedIn profile URL found in emails.".into());
            None
        }
        Err(e) => {
            tracing::warn!(error = %e, "[linkedin_enrichment] Gmail search failed");
            result.log.push(format!("Gmail search failed: {e}"));
            None
        }
    };

    result.profile_url = profile_url.clone();

    // ── Stage 2: scrape the LinkedIn profile via Apify ───────────────
    let Some(url) = profile_url else {
        result.log.push("Skipping LinkedIn scrape — no profile URL.".into());
        return Ok(result);
    };

    tracing::info!(url = %url, "[linkedin_enrichment] stage 2: scraping LinkedIn profile via Apify");
    result.log.push("Scraping LinkedIn profile...".into());

    match scrape_linkedin_profile(&client, &url).await {
        Ok(data) => {
            tracing::info!("[linkedin_enrichment] Apify scrape succeeded");
            result.log.push("LinkedIn profile scraped successfully.".into());

            // ── Stage 3: persist to memory ───────────────────────────
            tracing::info!("[linkedin_enrichment] stage 3: persisting profile to memory");
            if let Err(e) = persist_linkedin_profile(config, &url, &data).await {
                tracing::warn!(error = %e, "[linkedin_enrichment] failed to persist profile");
                result.log.push(format!("Failed to save profile: {e}"));
            } else {
                result.log.push("Profile saved to memory.".into());
            }

            result.profile_data = Some(data);
        }
        Err(e) => {
            tracing::warn!(error = %e, "[linkedin_enrichment] Apify scrape failed");
            result.log.push(format!("LinkedIn scrape failed: {e}"));

            // Still persist the URL even if the scrape failed — it's
            // useful context on its own.
            let _ = persist_linkedin_url_only(config, &url).await;
        }
    }

    Ok(result)
}

// ── Internal helpers ─────────────────────────────────────────────────

/// Search Gmail via Composio for emails from linkedin.com and extract
/// the user's own LinkedIn username.
///
/// LinkedIn notification emails embed `comm/in/<username>` links in the
/// **HTML body** — which Gmail returns as base64-encoded data inside
/// `payload.parts[].body.data`. We must decode those parts before
/// regex-matching; searching the raw JSON alone misses them.
async fn search_gmail_for_linkedin(config: &Config) -> anyhow::Result<Option<String>> {
    use crate::openhuman::composio::client::build_composio_client;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;

    let client = build_composio_client(config)
        .ok_or_else(|| anyhow::anyhow!("composio client unavailable"))?;

    // `comm/in/<username>` — LinkedIn's own notification emails always use
    // this form to refer to the email *recipient's* profile.
    static COMM_RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"linkedin\.com/comm/in/([a-zA-Z0-9_-]+)").unwrap()
    });

    let resp = client
        .execute_tool(
            "GMAIL_FETCH_EMAILS",
            Some(json!({
                "query": "from:linkedin.com",
                "max_results": 10,
            })),
        )
        .await
        .map_err(|e| anyhow::anyhow!("GMAIL_FETCH_EMAILS failed: {e:#}"))?;

    if !resp.successful {
        let err = resp.error.unwrap_or_else(|| "unknown error".into());
        anyhow::bail!("GMAIL_FETCH_EMAILS error: {err}");
    }

    // Walk the messages, decode HTML parts, and search for profile URLs.
    let messages = resp
        .data
        .get("messages")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    for msg in &messages {
        // Collect all text to search: plain messageText + decoded HTML parts.
        let mut searchable = String::new();

        // Plain text body (already decoded by Composio).
        if let Some(text) = msg.get("messageText").and_then(|v| v.as_str()) {
            searchable.push_str(text);
            searchable.push('\n');
        }

        // Decode base64 HTML parts from payload.parts[].body.data.
        if let Some(parts) = msg
            .pointer("/payload/parts")
            .and_then(|v| v.as_array())
        {
            for part in parts {
                let is_html = part
                    .get("mimeType")
                    .and_then(|v| v.as_str())
                    .map_or(false, |m| m.contains("html"));
                if !is_html {
                    continue;
                }
                if let Some(b64) = part.pointer("/body/data").and_then(|v| v.as_str()) {
                    if let Ok(bytes) = URL_SAFE_NO_PAD.decode(b64) {
                        if let Ok(html) = String::from_utf8(bytes) {
                            searchable.push_str(&html);
                            searchable.push('\n');
                        }
                    }
                }
            }
        }

        // Priority 1: comm/in/<username> — always the recipient's own profile.
        if let Some(caps) = COMM_RE.captures(&searchable) {
            let username = caps[1].to_string();
            let url = canonical_linkedin_url(&username);
            tracing::info!(
                username = %username,
                url = %url,
                "[linkedin_enrichment] found own username via comm/in/ in HTML body"
            );
            return Ok(Some(url));
        }

        // Priority 2: canonical /in/<username> (some notification types).
        if let Some(caps) = LINKEDIN_USERNAME_RE.captures(&searchable) {
            let username = caps[1].to_string();
            let url = canonical_linkedin_url(&username);
            tracing::info!(
                username = %username,
                url = %url,
                "[linkedin_enrichment] found username via /in/ in email body"
            );
            return Ok(Some(url));
        }
    }

    Ok(None)
}

/// Call the Apify LinkedIn profile scraper synchronously and return the
/// first profile item from the dataset.
async fn scrape_linkedin_profile(
    client: &Arc<IntegrationClient>,
    profile_url: &str,
) -> anyhow::Result<serde_json::Value> {
    let body = json!({
        "actorId": LINKEDIN_SCRAPER_ACTOR,
        "input": {
            "profileUrls": [profile_url],
        },
        "sync": true,
        "timeoutSecs": 120,
    });

    tracing::debug!(
        actor = LINKEDIN_SCRAPER_ACTOR,
        url = profile_url,
        "[linkedin_enrichment] invoking Apify actor"
    );

    // The backend wraps the Apify response in its standard envelope.
    // `IntegrationClient::post` already unwraps `{ success, data }`.
    let resp: serde_json::Value = client
        .post("/agent-integrations/apify/run", &body)
        .await
        .map_err(|e| anyhow::anyhow!("Apify run failed: {e:#}"))?;

    let status = resp
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("UNKNOWN");

    if status != "SUCCEEDED" {
        anyhow::bail!("Apify run finished with status: {status}");
    }

    // Extract the first item from the inline results array.
    let items = resp
        .get("items")
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow::anyhow!("Apify run returned no items array"))?;

    items
        .first()
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("Apify run returned an empty items array"))
}

/// Persist the full scraped LinkedIn profile to the user-profile memory
/// namespace so the agent has rich context about the user.
async fn persist_linkedin_profile(
    _config: &Config,
    url: &str,
    data: &serde_json::Value,
) -> anyhow::Result<()> {
    use crate::openhuman::memory::store::MemoryClient;

    let memory = MemoryClient::new_local()
        .map_err(|e| anyhow::anyhow!("memory client unavailable: {e}"))?;

    let content = format!(
        "LinkedIn profile for {url}:\n\n{}",
        serde_json::to_string_pretty(data).unwrap_or_else(|_| data.to_string())
    );

    memory
        .store_skill_sync(
            "user-profile",   // namespace skill_id
            "linkedin",       // integration_id
            &format!("LinkedIn profile: {url}"),
            &content,
            Some("onboarding-linkedin-enrichment".into()),
            Some(json!({
                "source": "apify-linkedin-scraper",
                "url": url,
                "actor": LINKEDIN_SCRAPER_ACTOR,
            })),
            Some("high".into()),
            None, // created_at
            None, // updated_at
            None, // document_id
        )
        .await
        .map_err(|e| anyhow::anyhow!("memory store failed: {e}"))
}

/// Fallback: persist just the LinkedIn URL when the full scrape fails.
async fn persist_linkedin_url_only(_config: &Config, url: &str) -> anyhow::Result<()> {
    use crate::openhuman::memory::store::MemoryClient;

    let memory = MemoryClient::new_local()
        .map_err(|e| anyhow::anyhow!("memory client unavailable: {e}"))?;

    memory
        .store_skill_sync(
            "user-profile",
            "linkedin",
            &format!("LinkedIn profile URL: {url}"),
            &format!("User LinkedIn profile: {url}"),
            Some("onboarding-linkedin-url".into()),
            Some(json!({ "source": "gmail-linkedin-extraction", "url": url })),
            Some("medium".into()),
            None, // created_at
            None, // updated_at
            None, // document_id
        )
        .await
        .map_err(|e| anyhow::anyhow!("memory store failed: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_username_from_canonical_url() {
        let text = "Check out https://www.linkedin.com/in/williamhgates for more";
        let caps = LINKEDIN_USERNAME_RE.captures(text).unwrap();
        assert_eq!(&caps[1], "williamhgates");
        assert_eq!(
            canonical_linkedin_url(&caps[1]),
            "https://www.linkedin.com/in/williamhgates"
        );
    }

    #[test]
    fn extracts_username_from_comm_url() {
        let text = "https://www.linkedin.com/comm/in/stevenenamakel?midToken=abc";
        let caps = LINKEDIN_USERNAME_RE.captures(text).unwrap();
        assert_eq!(&caps[1], "stevenenamakel");
        assert_eq!(
            canonical_linkedin_url(&caps[1]),
            "https://www.linkedin.com/in/stevenenamakel"
        );
    }

    #[test]
    fn extracts_username_from_http_variant() {
        let text = "See http://www.linkedin.com/in/jeannie-wyrick-b4760710a";
        let caps = LINKEDIN_USERNAME_RE.captures(text).unwrap();
        assert_eq!(&caps[1], "jeannie-wyrick-b4760710a");
    }

    #[test]
    fn skips_non_profile_linkedin_urls() {
        let text = "Visit https://www.linkedin.com/company/openai";
        assert!(LINKEDIN_USERNAME_RE.captures(text).is_none());
    }

    #[test]
    fn handles_no_match() {
        assert!(LINKEDIN_USERNAME_RE.captures("No LinkedIn here").is_none());
    }
}
