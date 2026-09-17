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

mod gmail_discovery;
mod memory_persistence;
mod profile_markdown;

use crate::config::Config;
use crate::integrations::build_client;
use regex::Regex;
use std::sync::LazyLock;

pub use gmail_discovery::scrape_linkedin_profile;
use gmail_discovery::search_gmail_for_linkedin;
use memory_persistence::{
    persist_linkedin_profile, persist_linkedin_url_only, profile_memory_writer,
};
pub use profile_markdown::{render_profile_markdown, summarise_profile_with_llm};
use profile_markdown::{write_profile_md, write_profile_md_url_only};

#[cfg(test)]
#[path = "linkedin_enrichment_tests.rs"]
mod tests;

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

/// Typed status for a pipeline stage.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum StageStatus {
    Success,
    Failed,
    Skipped,
}

/// A single pipeline stage result, suitable for structured RPC responses.
#[derive(Debug, Clone, serde::Serialize)]
pub struct EnrichmentStage {
    pub id: String,
    pub status: StageStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// Outcome of the full enrichment pipeline.
#[derive(Debug)]
pub struct LinkedInEnrichmentResult {
    /// The LinkedIn profile URL found in Gmail, if any.
    pub profile_url: Option<String>,
    /// Raw scraped profile JSON from Apify, if the scrape succeeded.
    pub profile_data: Option<serde_json::Value>,
    /// Typed stage results for structured consumption by the frontend.
    pub stages: Vec<EnrichmentStage>,
    /// Human-readable log lines for display.
    pub log: Vec<String>,
}

/// Run the full Gmail → LinkedIn → Apify enrichment pipeline.
///
/// `preset_profile_url` lets callers skip the Gmail-search stage and
/// supply a profile URL they already discovered out-of-band — currently
/// the frontend obtains one via the webview-driven
/// `gmail_find_linkedin_profile_url` Tauri command, which uses the
/// logged-in Gmail webview's CDP session instead of a Composio token.
/// When `None`, the function falls back to the Composio-driven Gmail
/// search at [`search_gmail_for_linkedin`] (which currently errors
/// because Composio Gmail was removed; callers should pass `Some` until
/// a Composio-free fallback ships).
///
/// Returns `Ok` with a result struct even if individual stages fail —
/// partial progress is still useful. Only returns `Err` if we can't
/// even build the integration client (i.e. user isn't signed in).
pub async fn run_linkedin_enrichment(
    config: &Config,
    preset_profile_url: Option<String>,
) -> anyhow::Result<LinkedInEnrichmentResult> {
    let mut result = LinkedInEnrichmentResult {
        profile_url: None,
        profile_data: None,
        stages: Vec::new(),
        log: Vec::new(),
    };

    // Short-circuit: if PROFILE.md is already on disk from a previous
    // enrichment run, skip the entire pipeline. The welcome agent reads
    // PROFILE.md straight from the workspace, so re-running stages 1-3
    // would just churn quota for the same output.
    let profile_path = config.workspace_dir.join("PROFILE.md");
    if profile_path.is_file() {
        tracing::info!(
            path = %profile_path.display(),
            "[linkedin_enrichment] PROFILE.md already exists — skipping pipeline"
        );
        result
            .log
            .push("PROFILE.md already exists — skipping enrichment.".into());
        for id in ["gmail-search", "apify-scrape", "build-profile"] {
            result.stages.push(EnrichmentStage {
                id: id.into(),
                status: StageStatus::Skipped,
                detail: Some("PROFILE.md already on disk".into()),
            });
        }
        return Ok(result);
    }

    let client = build_client(config)
        .ok_or_else(|| anyhow::anyhow!("no integration client — user not signed in"))?;

    // ── Stage 1: search Gmail for LinkedIn emails ───────────────────
    let profile_url = if let Some(url) = preset_profile_url {
        tracing::info!(url = %url, "[linkedin_enrichment] stage 1: using preset profile URL");
        result
            .log
            .push(format!("Using preset LinkedIn profile: {url}"));
        result.stages.push(EnrichmentStage {
            id: "gmail-search".into(),
            status: StageStatus::Success,
            detail: Some(url.clone()),
        });
        Some(url)
    } else {
        tracing::info!("[linkedin_enrichment] stage 1: searching Gmail for LinkedIn emails");
        result
            .log
            .push("Searching Gmail for LinkedIn emails...".into());
        match search_gmail_for_linkedin(config).await {
            Ok(Some(url)) => {
                tracing::info!(url = %url, "[linkedin_enrichment] found LinkedIn profile URL");
                result.log.push(format!("Found LinkedIn profile: {url}"));
                result.stages.push(EnrichmentStage {
                    id: "gmail-search".into(),
                    status: StageStatus::Success,
                    detail: Some(url.clone()),
                });
                Some(url)
            }
            Ok(None) => {
                tracing::info!("[linkedin_enrichment] no LinkedIn profile URL found in emails");
                result
                    .log
                    .push("No LinkedIn profile URL found in emails.".into());
                result.stages.push(EnrichmentStage {
                    id: "gmail-search".into(),
                    status: StageStatus::Skipped,
                    detail: Some("No LinkedIn profile URL found in emails".into()),
                });
                None
            }
            Err(e) => {
                tracing::warn!(error = %e, "[linkedin_enrichment] Gmail search failed");
                result.log.push(format!("Gmail search failed: {e}"));
                result.stages.push(EnrichmentStage {
                    id: "gmail-search".into(),
                    status: StageStatus::Failed,
                    detail: Some(format!("Gmail search failed: {e}")),
                });
                None
            }
        }
    };

    result.profile_url = profile_url.clone();

    // ── Stage 2: scrape the LinkedIn profile via Apify ───────────────
    let Some(url) = profile_url else {
        result
            .log
            .push("Skipping LinkedIn scrape — no profile URL.".into());
        result.stages.push(EnrichmentStage {
            id: "apify-scrape".into(),
            status: StageStatus::Skipped,
            detail: Some("No profile URL to scrape".into()),
        });
        result.stages.push(EnrichmentStage {
            id: "build-profile".into(),
            status: StageStatus::Skipped,
            detail: Some("No profile data".into()),
        });
        return Ok(result);
    };

    tracing::info!(url = %url, "[linkedin_enrichment] stage 2: scraping LinkedIn profile via Apify");
    result.log.push("Scraping LinkedIn profile...".into());

    // Resolve the guarded memory driver once for all persist calls.
    let memory = match profile_memory_writer().await {
        Ok(m) => Some(m),
        Err(e) => {
            tracing::warn!(
                error = %e,
                "[linkedin_enrichment] memory driver unavailable, skipping memory persistence"
            );
            None
        }
    };

    match scrape_linkedin_profile(&client, &url).await {
        Ok(data) => {
            tracing::info!("[linkedin_enrichment] Apify scrape succeeded");
            result
                .log
                .push("LinkedIn profile scraped successfully.".into());
            result.stages.push(EnrichmentStage {
                id: "apify-scrape".into(),
                status: StageStatus::Success,
                detail: None,
            });

            // ── Stage 3: write PROFILE.md to workspace ──────────────
            tracing::info!("[linkedin_enrichment] stage 3: writing PROFILE.md");
            if let Err(e) = write_profile_md(config, &url, &data).await {
                tracing::warn!(error = %e, "[linkedin_enrichment] failed to write PROFILE.md");
                result.log.push(format!("Failed to write PROFILE.md: {e}"));
                result.stages.push(EnrichmentStage {
                    id: "build-profile".into(),
                    status: StageStatus::Failed,
                    detail: Some(format!("{e}")),
                });
            } else {
                result.log.push("PROFILE.md written to workspace.".into());
                result.stages.push(EnrichmentStage {
                    id: "build-profile".into(),
                    status: StageStatus::Success,
                    detail: Some("PROFILE.md written".into()),
                });
            }

            // Also persist to memory store for RAG retrieval.
            if let Some(ref mem) = memory {
                if let Err(e) = persist_linkedin_profile(mem, &url, &data).await {
                    tracing::warn!(error = %e, "[linkedin_enrichment] failed to persist to memory");
                }
            }

            result.profile_data = Some(data);
        }
        Err(e) => {
            tracing::warn!(error = %e, "[linkedin_enrichment] Apify scrape failed");
            result.log.push(format!("LinkedIn scrape failed: {e}"));
            result.stages.push(EnrichmentStage {
                id: "apify-scrape".into(),
                status: StageStatus::Failed,
                detail: Some(format!("{e}")),
            });
            result.stages.push(EnrichmentStage {
                id: "build-profile".into(),
                status: StageStatus::Skipped,
                detail: Some("Scrape failed".into()),
            });

            // Still write a minimal PROFILE.md with just the URL.
            if let Err(e) = write_profile_md_url_only(config, &url) {
                tracing::warn!(error = %e, "[linkedin_enrichment] failed to write PROFILE.md");
            }
            if let Some(ref mem) = memory {
                let _ = persist_linkedin_url_only(mem, &url).await;
            }
        }
    }

    Ok(result)
}
