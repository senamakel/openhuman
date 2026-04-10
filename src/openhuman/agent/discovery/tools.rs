//! Tool definitions + dispatchers for the discovery agent.
//!
//! The discovery agent exposes three tools to the LLM:
//!
//! | Tool name             | What it does                                                |
//! |-----------------------|-------------------------------------------------------------|
//! | `owner_write`         | Persist structured facts and/or a rich document about the owner. Same write path as `memory.updateOwner` in skills. |
//! | `apify_search_person` | Call the configured Apify "person search" actor via the backend proxy and return dataset items. |
//! | `apify_fetch_url`     | Call the configured Apify "fetch URL" actor to scrape a page and return its content. |
//!
//! All tool specs are rendered in OpenAI tool-schema format and
//! returned from [`tool_specs`]. Call-dispatch happens in
//! [`dispatch_tool`].

use serde_json::{json, Value};
use std::sync::Arc;

use crate::openhuman::agent::discovery::apify::{ApifyClient, ApifyError};
use crate::openhuman::agent::discovery::types::DiscoveryReport;
use crate::openhuman::config::DiscoveryConfig;
use crate::openhuman::memory::store::profile::FacetType;
use crate::openhuman::memory::MemoryClient;

/// Result of dispatching a single tool call.
pub struct ToolOutcome {
    /// String content appended to the chat history as the `tool`
    /// role message. Should be short enough to fit alongside the next
    /// round's prompt.
    pub content: String,
    /// Whether the call succeeded. Errors are still returned as
    /// content (so the model can see and react) but counted separately.
    pub success: bool,
}

/// Build the OpenAI-format tool specs array the discovery agent
/// exposes to the LLM on each round.
pub fn tool_specs() -> Vec<Value> {
    vec![
        json!({
            "type": "function",
            "function": {
                "name": "owner_write",
                "description": "Persist identity facts or a rich document about the owner of this OpenHuman instance. Facts land in the user_profile table; documents land in the `owner` memory namespace. Use this ONLY for high-confidence information you are sure about.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "facts": {
                            "type": "array",
                            "description": "Structured facts. Each fact is a triple of (type, key, value). Use type='identity' for hard biographical data (name, email, company, location, timezone), 'role' for job title, 'preference' for lifestyle/tool preferences.",
                            "items": {
                                "type": "object",
                                "required": ["type", "key", "value"],
                                "properties": {
                                    "type": {
                                        "type": "string",
                                        "enum": ["identity", "preference", "skill", "role", "personality", "context"]
                                    },
                                    "key": { "type": "string" },
                                    "value": { "type": "string" },
                                    "confidence": {
                                        "type": "number",
                                        "minimum": 0.0,
                                        "maximum": 1.0
                                    }
                                }
                            }
                        },
                        "document": {
                            "type": "object",
                            "description": "Optional long-form blob (bio, summary, resume paragraph).",
                            "required": ["title", "content"],
                            "properties": {
                                "title": { "type": "string" },
                                "content": { "type": "string" }
                            }
                        }
                    }
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "apify_search_person",
                "description": "Search for a person using an Apify actor (LinkedIn-style scraper) via the authenticated backend proxy. Returns dataset items. Call this FIRST with any seed you have (name, email, or company) to find candidate profiles.",
                "parameters": {
                    "type": "object",
                    "required": ["query"],
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "Search query — a name, email, or 'name at company'."
                        },
                        "max_results": {
                            "type": "integer",
                            "minimum": 1,
                            "maximum": 20,
                            "description": "Upper bound on returned items. Default 5."
                        }
                    }
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "apify_fetch_url",
                "description": "Fetch and scrape a single URL via an Apify web-scraper actor through the authenticated backend proxy. Use this to pull a bio, about-page, or profile page surfaced by `apify_search_person`.",
                "parameters": {
                    "type": "object",
                    "required": ["url"],
                    "properties": {
                        "url": {
                            "type": "string",
                            "description": "Absolute HTTPS URL to fetch."
                        }
                    }
                }
            }
        }),
    ]
}

/// Dispatch a single tool call by name. Unknown tool names return an
/// error outcome (not a fatal runner error — the loop may still
/// converge).
pub async fn dispatch_tool(
    name: &str,
    arguments: &Value,
    memory: &MemoryClient,
    apify: &Arc<dyn ApifyClient>,
    config: &DiscoveryConfig,
    report: &mut DiscoveryReport,
    origin: &str,
) -> ToolOutcome {
    match name {
        "owner_write" => owner_write(arguments, memory, report, origin).await,
        "apify_search_person" => apify_search_person(arguments, apify, config).await,
        "apify_fetch_url" => apify_fetch_url(arguments, apify, config).await,
        other => ToolOutcome {
            content: format!("unknown tool '{other}'"),
            success: false,
        },
    }
}

async fn owner_write(
    arguments: &Value,
    memory: &MemoryClient,
    report: &mut DiscoveryReport,
    origin: &str,
) -> ToolOutcome {
    let obj = match arguments.as_object() {
        Some(o) => o,
        None => {
            return ToolOutcome {
                content: "owner_write: arguments must be an object".into(),
                success: false,
            };
        }
    };

    let facts = obj
        .get("facts")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut written_facts = 0usize;
    for (idx, fact) in facts.iter().enumerate() {
        let ft = fact.get("type").and_then(Value::as_str).unwrap_or("");
        let key = fact.get("key").and_then(Value::as_str).unwrap_or("");
        let value = fact.get("value").and_then(Value::as_str).unwrap_or("");
        let confidence = fact.get("confidence").and_then(Value::as_f64);

        let facet_type = match ft.to_ascii_lowercase().as_str() {
            "identity" => FacetType::Identity,
            "preference" => FacetType::Preference,
            "skill" => FacetType::Skill,
            "role" => FacetType::Role,
            "personality" => FacetType::Personality,
            "context" => FacetType::Context,
            other => {
                report
                    .errors
                    .push(format!("owner_write: facts[{idx}]: unknown type '{other}'"));
                continue;
            }
        };

        if key.trim().is_empty() || value.trim().is_empty() {
            report
                .errors
                .push(format!("owner_write: facts[{idx}]: key/value empty"));
            continue;
        }

        if let Err(e) = memory.profile_upsert_owner(facet_type, key, value, confidence, origin) {
            report
                .errors
                .push(format!("owner_write: facts[{idx}]: {e}"));
            continue;
        }
        written_facts += 1;
    }

    let mut doc_written = false;
    if let Some(doc) = obj.get("document").and_then(Value::as_object) {
        let title = doc.get("title").and_then(Value::as_str).unwrap_or("");
        let content = doc.get("content").and_then(Value::as_str).unwrap_or("");
        if title.trim().is_empty() || content.trim().is_empty() {
            report
                .errors
                .push("owner_write: document: title/content empty".into());
        } else {
            match memory
                .store_owner_doc(title, content, Some("doc".into()), origin)
                .await
            {
                Ok(_) => {
                    doc_written = true;
                    report.docs_written += 1;
                }
                Err(e) => {
                    report.errors.push(format!("owner_write: document: {e}"));
                }
            }
        }
    }

    report.facts_written += written_facts;

    ToolOutcome {
        content: format!(
            "owner_write ok: {} fact(s){} stored",
            written_facts,
            if doc_written { " + 1 document" } else { "" }
        ),
        success: true,
    }
}

async fn apify_search_person(
    arguments: &Value,
    apify: &Arc<dyn ApifyClient>,
    config: &DiscoveryConfig,
) -> ToolOutcome {
    let query = match arguments.get("query").and_then(Value::as_str) {
        Some(q) if !q.trim().is_empty() => q.to_string(),
        _ => {
            return ToolOutcome {
                content: "apify_search_person: missing 'query'".into(),
                success: false,
            };
        }
    };
    let max_results = arguments
        .get("max_results")
        .and_then(Value::as_u64)
        .unwrap_or(5);

    let actor_input = json!({
        "query": query,
        "maxResults": max_results,
    });

    match apify
        .run_actor(
            &config.person_search_actor,
            actor_input,
            config.actor_timeout_secs,
        )
        .await
    {
        Ok(res) => ToolOutcome {
            content: format_items_for_llm("apify_search_person", &res.items),
            success: true,
        },
        Err(e) => ToolOutcome {
            content: format!("apify_search_person failed: {}", format_apify_error(e)),
            success: false,
        },
    }
}

async fn apify_fetch_url(
    arguments: &Value,
    apify: &Arc<dyn ApifyClient>,
    config: &DiscoveryConfig,
) -> ToolOutcome {
    let url = match arguments.get("url").and_then(Value::as_str) {
        Some(u) if !u.trim().is_empty() => u.to_string(),
        _ => {
            return ToolOutcome {
                content: "apify_fetch_url: missing 'url'".into(),
                success: false,
            };
        }
    };

    let actor_input = json!({
        "startUrls": [{ "url": url }],
        "maxPages": 1,
    });

    match apify
        .run_actor(
            &config.fetch_url_actor,
            actor_input,
            config.actor_timeout_secs,
        )
        .await
    {
        Ok(res) => ToolOutcome {
            content: format_items_for_llm("apify_fetch_url", &res.items),
            success: true,
        },
        Err(e) => ToolOutcome {
            content: format!("apify_fetch_url failed: {}", format_apify_error(e)),
            success: false,
        },
    }
}

/// Condense Apify dataset items into something short enough to fit in
/// the next LLM round without blowing the context budget. Emits the
/// first 3 items as pretty-printed JSON, truncated to 2k chars.
fn format_items_for_llm(tool_name: &str, items: &[Value]) -> String {
    if items.is_empty() {
        return format!("{tool_name}: 0 items");
    }
    let head: Vec<&Value> = items.iter().take(3).collect();
    let mut out = format!("{tool_name}: {} items\n", items.len());
    let pretty = serde_json::to_string_pretty(&head).unwrap_or_else(|_| "[]".into());
    if pretty.chars().count() > 2000 {
        let truncated: String = pretty.chars().take(2000).collect();
        out.push_str(&truncated);
        out.push_str("\n… [truncated]");
    } else {
        out.push_str(&pretty);
    }
    out
}

fn format_apify_error(e: ApifyError) -> String {
    match e {
        ApifyError::Http(m) => format!("http: {m}"),
        ApifyError::NonSuccess(s) => format!("non-success status: {s}"),
        ApifyError::MissingField(f) => format!("missing field: {f}"),
    }
}
