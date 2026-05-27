//! GitHub repo source reader.
//!
//! Lists files from a public GitHub repository via the GitHub API
//! (no OAuth required) and reads individual file content.

use async_trait::async_trait;
use serde::Deserialize;

use crate::openhuman::config::Config;
use crate::openhuman::memory_sources::types::{
    ContentType, MemorySourceEntry, SourceContent, SourceItem, SourceKind,
};

use super::SourceReader;

const DEFAULT_BRANCH: &str = "main";
const GITHUB_API_BASE: &str = "https://api.github.com";

pub struct GithubReader;

/// Parse `owner` and `repo` from a GitHub URL.
fn parse_github_url(url: &str) -> Result<(String, String), String> {
    let cleaned = url
        .trim_end_matches('/')
        .trim_end_matches(".git");
    let parts: Vec<&str> = cleaned.rsplitn(3, '/').collect();
    if parts.len() < 2 {
        return Err(format!("cannot parse GitHub owner/repo from: {url}"));
    }
    Ok((parts[1].to_string(), parts[0].to_string()))
}

#[derive(Debug, Deserialize)]
struct TreeResponse {
    tree: Vec<TreeEntry>,
}

#[derive(Debug, Deserialize)]
struct TreeEntry {
    path: String,
    #[serde(rename = "type")]
    entry_type: String,
    #[allow(dead_code)]
    size: Option<u64>,
}

#[async_trait]
impl SourceReader for GithubReader {
    fn kind(&self) -> SourceKind {
        SourceKind::GithubRepo
    }

    async fn list_items(
        &self,
        source: &MemorySourceEntry,
        _config: &Config,
    ) -> Result<Vec<SourceItem>, String> {
        let url = source.url.as_deref().ok_or("github source requires a url")?;
        let (owner, repo) = parse_github_url(url)?;
        let branch = source.branch.as_deref().unwrap_or(DEFAULT_BRANCH);
        let path_filters = &source.paths;

        tracing::debug!(
            owner = %owner,
            repo = %repo,
            branch = %branch,
            "[memory_sources:github] listing items"
        );

        let api_url = format!(
            "{GITHUB_API_BASE}/repos/{owner}/{repo}/git/trees/{branch}?recursive=1"
        );

        let client = reqwest::Client::new();
        let resp = client
            .get(&api_url)
            .header("User-Agent", "openhuman")
            .header("Accept", "application/vnd.github.v3+json")
            .send()
            .await
            .map_err(|e| format!("GitHub API request failed: {e}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("GitHub API returned {status}: {body}"));
        }

        let tree_resp: TreeResponse = resp
            .json()
            .await
            .map_err(|e| format!("failed to parse GitHub tree response: {e}"))?;

        let items: Vec<SourceItem> = tree_resp
            .tree
            .into_iter()
            .filter(|e| e.entry_type == "blob")
            .filter(|e| is_readable_file(&e.path))
            .filter(|e| {
                if path_filters.is_empty() {
                    return true;
                }
                path_filters.iter().any(|filter| e.path.starts_with(filter))
            })
            .map(|e| SourceItem {
                id: e.path.clone(),
                title: e.path,
                updated_at_ms: None,
            })
            .collect();

        tracing::debug!(
            count = items.len(),
            "[memory_sources:github] found items"
        );

        Ok(items)
    }

    async fn read_item(
        &self,
        source: &MemorySourceEntry,
        item_id: &str,
        _config: &Config,
    ) -> Result<SourceContent, String> {
        let url = source.url.as_deref().ok_or("github source requires a url")?;
        let (owner, repo) = parse_github_url(url)?;
        let branch = source.branch.as_deref().unwrap_or(DEFAULT_BRANCH);

        let raw_url = format!(
            "https://raw.githubusercontent.com/{owner}/{repo}/{branch}/{item_id}"
        );

        tracing::debug!(
            path = %item_id,
            "[memory_sources:github] reading item"
        );

        let client = reqwest::Client::new();
        let resp = client
            .get(&raw_url)
            .header("User-Agent", "openhuman")
            .send()
            .await
            .map_err(|e| format!("failed to fetch {item_id}: {e}"))?;

        if !resp.status().is_success() {
            return Err(format!(
                "GitHub raw fetch returned {}: {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            ));
        }

        let body = resp
            .text()
            .await
            .map_err(|e| format!("failed to read response body: {e}"))?;

        let content_type = if item_id.ends_with(".md") {
            ContentType::Markdown
        } else if item_id.ends_with(".html") || item_id.ends_with(".htm") {
            ContentType::Html
        } else {
            ContentType::Plaintext
        };

        Ok(SourceContent {
            id: item_id.to_string(),
            title: item_id.to_string(),
            body,
            content_type,
            metadata: serde_json::json!({
                "owner": owner,
                "repo": repo,
                "branch": branch,
            }),
        })
    }
}

fn is_readable_file(path: &str) -> bool {
    let readable_extensions = [
        ".md", ".txt", ".rst", ".adoc", ".org", ".json", ".yaml", ".yml",
        ".toml", ".xml", ".csv", ".rs", ".py", ".js", ".ts", ".go", ".java",
        ".c", ".h", ".cpp", ".hpp", ".rb", ".sh", ".html", ".htm", ".css",
    ];
    readable_extensions.iter().any(|ext| path.ends_with(ext))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_github_url_extracts_owner_and_repo() {
        let (owner, repo) = parse_github_url("https://github.com/openai/tiktoken").unwrap();
        assert_eq!(owner, "openai");
        assert_eq!(repo, "tiktoken");
    }

    #[test]
    fn parse_github_url_handles_trailing_slash_and_git() {
        let (owner, repo) =
            parse_github_url("https://github.com/org/repo.git/").unwrap();
        assert_eq!(owner, "org");
        assert_eq!(repo, "repo");
    }

    #[test]
    fn is_readable_file_accepts_common_extensions() {
        assert!(is_readable_file("README.md"));
        assert!(is_readable_file("src/main.rs"));
        assert!(!is_readable_file("image.png"));
        assert!(!is_readable_file("binary.exe"));
    }
}
