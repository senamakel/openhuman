//! Tavily wire types (search/extract responses) and small string-formatting
//! helpers shared by the client and both tools.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

const IMAGE_DESCRIPTION_MAX_CHARS: usize = 300;

/// Tavily may return image entries as a bare URL or as an object with an
/// optional description, depending on the endpoint options and API version.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum TavilyImage {
    Url(String),
    Detailed {
        #[serde(default)]
        url: String,
        #[serde(default)]
        description: Option<String>,
    },
}

impl TavilyImage {
    pub(super) fn url(&self) -> &str {
        match self {
            Self::Url(url) | Self::Detailed { url, .. } => url,
        }
    }

    pub(super) fn description(&self) -> Option<String> {
        let description = match self {
            Self::Url(_) => None,
            Self::Detailed { description, .. } => non_empty(description.as_deref()),
        }?;
        let single_line = description.replace(['\r', '\n'], " ");
        Some(crate::util::truncate_with_ellipsis(
            &single_line,
            IMAGE_DESCRIPTION_MAX_CHARS,
        ))
    }
}

/// One Tavily search result, shared by `/search`.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct TavilyResultItem {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub score: Option<f64>,
    #[serde(default, rename = "raw_content")]
    pub raw_content: Option<String>,
    #[serde(default)]
    pub images: Vec<TavilyImage>,
}

impl TavilyResultItem {
    /// Best available excerpt: the chunked `content` first, then the full
    /// cleaned page (`raw_content`), only present when `include_raw_content`
    /// was requested.
    pub(super) fn excerpt(&self) -> Option<String> {
        non_empty(self.content.as_deref()).or_else(|| non_empty(self.raw_content.as_deref()))
    }

    pub(super) fn display_title(&self) -> &str {
        self.title
            .as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .unwrap_or("Untitled")
    }
}

#[derive(Debug, Default, Deserialize, Serialize)]
pub struct TavilySearchResponse {
    #[serde(default)]
    pub query: Option<String>,
    #[serde(default)]
    pub answer: Option<String>,
    #[serde(default)]
    pub images: Vec<TavilyImage>,
    #[serde(default)]
    pub results: Vec<TavilyResultItem>,
}

/// One URL's extracted content from `/extract`.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct TavilyExtractResult {
    #[serde(default)]
    pub url: String,
    #[serde(default, rename = "raw_content")]
    pub raw_content: Option<String>,
}

#[derive(Debug, Default, Deserialize, Serialize)]
pub struct TavilyExtractResponse {
    #[serde(default)]
    pub results: Vec<TavilyExtractResult>,
    #[serde(default, rename = "failed_results")]
    pub failed_results: Vec<TavilyExtractFailure>,
}

/// A URL Tavily could not process. Errors are rendered but never attributed to
/// a template — the `error` string is remote-controlled and must not be trusted.
#[derive(Debug, Default, Deserialize, Serialize)]
pub struct TavilyExtractFailure {
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
}

pub(super) fn non_empty(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// Escape a remote page title for use as a markdown link label. Titles are
/// attacker-controlled, and an unescaped `[`/`]` breaks out of the link and
/// lets a crafted page inject markdown into the agent transcript.
pub(super) fn escape_link_text(raw: &str) -> String {
    raw.replace('\\', r"\\")
        .replace('[', r"\[")
        .replace(']', r"\]")
}

/// Render a URL as a markdown link destination. Bare parentheses (common in
/// Wikipedia URLs) terminate the destination early, so wrap in angle brackets
/// and drop the characters that would close them.
pub(super) fn escape_link_destination(raw: &str) -> String {
    let cleaned: String = raw
        .trim()
        .chars()
        .filter(|c| !matches!(c, '<' | '>' | ' '))
        .collect();
    format!("<{cleaned}>")
}

/// Copy a string array argument onto the Tavily request body.
pub(super) fn copy_domain_filter(args: &Value, from: &str, body: &mut Value) {
    if let Some(list) = args.get(from).filter(|v| v.is_array()) {
        body[from] = list.clone();
    }
}

/// Copy an optional string argument onto the Tavily request body under the
/// same (already snake_case) key.
pub(super) fn copy_string(args: &Value, key: &str, body: &mut Value) {
    if let Some(value) = non_empty(args.get(key).and_then(Value::as_str)) {
        body[key] = json!(value);
    }
}

/// Copy an optional boolean argument onto the Tavily request body.
pub(super) fn copy_bool(args: &Value, key: &str, body: &mut Value) {
    if let Some(value) = args.get(key).and_then(Value::as_bool) {
        body[key] = json!(value);
    }
}
