//! Profile discovery: Gmail search for the user's own LinkedIn URL and the
//! Apify profile scrape.

use crate::config::Config;
use crate::integrations::IntegrationClient;
use regex::Regex;
use serde_json::json;
use std::sync::{Arc, LazyLock};

use super::{canonical_linkedin_url, LINKEDIN_SCRAPER_ACTOR, LINKEDIN_USERNAME_RE};

/// Search Gmail via Composio for emails from linkedin.com and extract
/// the user's own LinkedIn username.
///
/// LinkedIn notification emails embed `comm/in/<username>` links in the
/// **HTML body** — which Gmail returns as base64-encoded data inside
/// `payload.parts[].body.data`. We must decode those parts before
/// regex-matching; searching the raw JSON alone misses them.
pub(super) async fn search_gmail_for_linkedin(config: &Config) -> anyhow::Result<Option<String>> {
    use crate::integrations::composio::client::{
        create_composio_client, direct_execute, ComposioClientKind,
    };
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;

    // Resolve through the mode-aware factory so a direct-mode user
    // with a stored API key can still drive Gmail enrichment from the
    // personal Composio tenant (#1710 Wave 2). Pre-fix this path used
    // `build_composio_client` and returned early for any user without
    // a backend session, silently disabling LinkedIn enrichment for
    // direct-mode users even when their LinkedIn/Gmail connections
    // were healthy on app.composio.dev.
    let client_kind = create_composio_client(config)
        .map_err(|e| anyhow::anyhow!("composio client unavailable: {e}"))?;

    // `comm/in/<username>` — LinkedIn's own notification emails always use
    // this form to refer to the email *recipient's* profile.
    static COMM_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"linkedin\.com/comm/in/([a-zA-Z0-9_-]+)").unwrap());

    let args = json!({
        "query": "from:linkedin.com",
        "max_results": 10,
    });
    let resp = match &client_kind {
        ComposioClientKind::Backend(client) => client
            .execute_tool("GMAIL_FETCH_EMAILS", Some(args))
            .await
            .map_err(|e| anyhow::anyhow!("GMAIL_FETCH_EMAILS failed: {e:#}"))?,
        ComposioClientKind::Direct(direct) => {
            tracing::debug!(
                "[linkedin_enrichment][composio-direct] GMAIL_FETCH_EMAILS via direct tenant"
            );
            direct_execute(
                direct,
                "GMAIL_FETCH_EMAILS",
                Some(args),
                &config.composio.entity_id,
                None,
            )
            .await
            .map_err(|e| anyhow::anyhow!("GMAIL_FETCH_EMAILS (direct) failed: {e:#}"))?
        }
    };

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
        if let Some(parts) = msg.pointer("/payload/parts").and_then(|v| v.as_array()) {
            for part in parts {
                let is_html = part
                    .get("mimeType")
                    .and_then(|v| v.as_str())
                    .is_some_and(|m| m.contains("html"));
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
pub async fn scrape_linkedin_profile(
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
        url_len = profile_url.len(),
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
