//! `PROFILE.md` generation: rendering the scraped LinkedIn payload as
//! Markdown and distilling it through the backend LLM.

use crate::config::Config;

// ── PROFILE.md generation ────────────────────────────────────────────

/// Summarise the scraped LinkedIn data with an LLM, then write the
/// result to `{workspace_dir}/PROFILE.md`. The prompt system picks this
/// file up automatically on the next agent turn.
pub(super) async fn write_profile_md(
    config: &Config,
    url: &str,
    data: &serde_json::Value,
) -> anyhow::Result<()> {
    // First render a full Markdown draft from the raw data.
    let raw_md = render_profile_markdown(url, data);

    // Then compress it through the LLM.
    let md = match summarise_profile_with_llm(config, &raw_md).await {
        Ok(summary) => summary,
        Err(e) => {
            tracing::warn!(
                error = %e,
                "[linkedin_enrichment] LLM summarisation failed, falling back to raw markdown"
            );
            raw_md
        }
    };

    let path = config.workspace_dir.join("PROFILE.md");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, &md)?;
    tracing::info!(path = %path.display(), len = md.len(), "[linkedin_enrichment] wrote PROFILE.md");
    Ok(())
}

/// Ask the backend LLM to distil the raw LinkedIn Markdown into a
/// concise, high-signal profile document suitable for agent context.
pub async fn summarise_profile_with_llm(config: &Config, raw_md: &str) -> anyhow::Result<String> {
    let (model_chat, _) = crate::inference::provider::create_chat_model_from_string_with_model_id(
        "summarization",
        "openhuman",
        config,
        0.3,
    )?;

    let system = "\
You are a profile analyst. You will receive a user's LinkedIn profile in Markdown format. \
Your job is to produce a concise PROFILE.md that an AI assistant will read to understand \
who this user is.\n\n\
Rules:\n\
- Output clean Markdown with a `# User Profile` heading.\n\
- Lead with name, headline, location, and LinkedIn URL.\n\
- Summarise the About section in 2-3 sentences max.\n\
- List only the most notable experiences (founder roles, leadership positions) — skip \
  short stints and minor roles.\n\
- Include education, languages, and any standout achievements.\n\
- Add a short `## Key facts for the assistant` section with 5-8 bullet points the AI \
  should know (e.g. expertise areas, industries, current focus, communication style hints).\n\
- Keep the entire output under 400 words.\n\
- Do not invent information — only use what is in the input.";

    let model = "summarization-v1";

    tracing::debug!(
        model = model,
        input_len = raw_md.len(),
        "[linkedin_enrichment] sending profile to LLM for summarisation"
    );

    use tinyinference::message::Message;
    use tinyinference::model::ModelRequest;
    let summary = model_chat
        .invoke(
            &(),
            ModelRequest::new(vec![
                Message::system(system),
                Message::user(raw_md.to_string()),
            ]),
        )
        .await?
        .text();

    tracing::debug!(
        output_len = summary.len(),
        "[linkedin_enrichment] LLM summarisation complete"
    );

    Ok(summary)
}

/// Minimal fallback when the Apify scrape failed but we have the URL.
pub(super) fn write_profile_md_url_only(config: &Config, url: &str) -> anyhow::Result<()> {
    let md = format!(
        "# User Profile\n\n\
         LinkedIn: {url}\n\n\
         _Full profile data was not available at onboarding time._\n"
    );
    let path = config.workspace_dir.join("PROFILE.md");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, md)?;
    Ok(())
}

/// Turn the Apify scrape JSON into clean Markdown.
pub fn render_profile_markdown(url: &str, data: &serde_json::Value) -> String {
    let s = |key: &str| {
        data.get(key)
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    };

    let full_name = s("fullName");
    let headline = s("headline");
    let location = s("addressWithCountry");
    let about = s("about");
    let connections = data.get("connections").and_then(|v| v.as_u64());
    let followers = data.get("followers").and_then(|v| v.as_u64());

    let mut md = format!("# User Profile — {full_name}\n\n");

    if !headline.is_empty() {
        md.push_str(&format!("**{headline}**\n\n"));
    }
    if !location.is_empty() {
        md.push_str(&format!("Location: {location}\n\n"));
    }
    md.push_str(&format!("LinkedIn: {url}\n\n"));
    if let (Some(c), Some(f)) = (connections, followers) {
        md.push_str(&format!("Connections: {c} | Followers: {f}\n\n"));
    }

    if !about.is_empty() {
        md.push_str("## About\n\n");
        md.push_str(&about);
        md.push_str("\n\n");
    }

    // Experience
    if let Some(exps) = data.get("experiences").and_then(|v| v.as_array()) {
        if !exps.is_empty() {
            md.push_str("## Experience\n\n");
            for exp in exps {
                let title = exp.get("title").and_then(|v| v.as_str()).unwrap_or("");
                let company = exp.get("subtitle").and_then(|v| v.as_str()).unwrap_or("");
                let duration = exp.get("duration").and_then(|v| v.as_str()).unwrap_or("");
                let caption = exp.get("caption").and_then(|v| v.as_str()).unwrap_or("");
                let desc = exp
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                md.push_str(&format!("- **{title}**"));
                if !company.is_empty() {
                    md.push_str(&format!(" at {company}"));
                }
                if !duration.is_empty() {
                    md.push_str(&format!(" ({duration})"));
                }
                if !caption.is_empty() {
                    md.push_str(&format!(" — {caption}"));
                }
                md.push('\n');
                if !desc.is_empty() {
                    md.push_str(&format!("  {desc}\n"));
                }
            }
            md.push('\n');
        }
    }

    // Education
    if let Some(edus) = data.get("educations").and_then(|v| v.as_array()) {
        if !edus.is_empty() {
            md.push_str("## Education\n\n");
            for edu in edus {
                let school = edu.get("title").and_then(|v| v.as_str()).unwrap_or("");
                let degree = edu.get("subtitle").and_then(|v| v.as_str()).unwrap_or("");
                md.push_str(&format!("- **{school}**"));
                if !degree.is_empty() {
                    md.push_str(&format!(" — {degree}"));
                }
                md.push('\n');
            }
            md.push('\n');
        }
    }

    // Languages
    if let Some(langs) = data.get("languages").and_then(|v| v.as_array()) {
        if !langs.is_empty() {
            let names: Vec<&str> = langs
                .iter()
                .filter_map(|l| l.get("name").and_then(|v| v.as_str()))
                .collect();
            if !names.is_empty() {
                md.push_str(&format!("Languages: {}\n\n", names.join(", ")));
            }
        }
    }

    // Volunteering
    if let Some(vols) = data.get("volunteering").and_then(|v| v.as_array()) {
        if !vols.is_empty() {
            md.push_str("## Volunteering\n\n");
            for vol in vols {
                let title = vol.get("title").and_then(|v| v.as_str()).unwrap_or("");
                let org = vol.get("subtitle").and_then(|v| v.as_str()).unwrap_or("");
                md.push_str(&format!("- {title}"));
                if !org.is_empty() {
                    md.push_str(&format!(" at {org}"));
                }
                md.push('\n');
            }
            md.push('\n');
        }
    }

    md
}
