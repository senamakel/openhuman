//! Where a catalog entry's `SKILL.md` lives, and the URL to fetch it from.
//!
//! [`derive_download_url`] picks the download URL for an entry at index time;
//! the rest of this module is the per-source knowledge it draws on.
//!
//! - **ClawHub** entries carry only a slug; ClawHub's file API serves the raw
//!   `SKILL.md` for it.
//! - **skills.sh** entries point at `skills.sh/<owner>/<repo>/<skill>`, a
//!   listing of a GitHub repo. Repos keep skills in different directories, so
//!   the file is located at install time: the conventional directories first,
//!   then one recursive tree listing of the repo.
//!
//! LobeHub entries are system-prompt agents with no `SKILL.md` at all and stay
//! uninstallable.

use std::time::Duration;

use serde_json::Value;

const DOWNLOAD_BASE_URL_ENV: &str = "OPENHUMAN_SKILL_REGISTRY_DOWNLOAD_BASE_URL";
const CLAWHUB_SKILLS_API: &str = "https://clawhub.ai/api/v1/skills";
const GITHUB_RAW: &str = "https://raw.githubusercontent.com";
const GITHUB_REPOS_API: &str = "https://api.github.com/repos";
/// Directories a skills.sh repo conventionally keeps a skill under, probed in
/// this order before listing the whole repo.
const SKILLS_SH_BASE_DIRS: [&str; 4] = ["", "skills/", ".agents/skills/", ".claude/skills/"];
const PROBE_TIMEOUT_SECS: u64 = 15;

/// Resolve a fetchable `SKILL.md` URL for a catalog entry.
///
/// Precedence:
/// 1. `OPENHUMAN_SKILL_REGISTRY_DOWNLOAD_BASE_URL` test override.
/// 2. `docsPath` — Hermes' own bundled / optional skills, which live in the
///    `NousResearch/hermes-agent` repo under `skills/` / `optional-skills/`.
/// 3. `sourceUrl` on GitHub (browse.sh, NVIDIA, GitHub, ...): the blob/tree
///    view is rewritten to the `raw.githubusercontent.com` `SKILL.md`.
/// 4. ClawHub `identifier`: ClawHub's file API by slug.
/// 5. skills.sh `sourceUrl`: the most common location in the listed GitHub
///    repo. `install_from_catalog` locates the real one before fetching.
///
/// Returns an empty string when no download exists (LobeHub agents have no
/// `SKILL.md`). `install_from_catalog` turns that into an actionable error
/// rather than fetching a guaranteed-404 URL (#3741).
pub(super) fn derive_download_url(
    source: &str,
    identifier: Option<&str>,
    name: &str,
    docs_path: Option<&str>,
    source_url: Option<&str>,
) -> String {
    if let Ok(base) = std::env::var(DOWNLOAD_BASE_URL_ENV) {
        let base = base.trim().trim_end_matches('/');
        if !base.is_empty() {
            return format!("{base}/{name}/SKILL.md");
        }
    }
    if let Some(url) = docs_path.and_then(download_url_from_docs_path) {
        return url;
    }
    if let Some(url) = source_url.and_then(download_url_from_source_url) {
        return url;
    }
    if source.eq_ignore_ascii_case("clawhub") {
        if let Some(url) = identifier.and_then(clawhub_download_url) {
            return url;
        }
    }
    if let Some(skill) = source_url.and_then(SkillsShRef::parse) {
        if let Some(url) = skill.candidate_urls().into_iter().next() {
            return url;
        }
    }
    String::new()
}

/// Rewrite a GitHub `sourceUrl` (blob or tree view) into the raw
/// `SKILL.md` download URL. Returns `None` for non-GitHub hosts (portal pages
/// that serve HTML, not raw markdown).
///
/// - blob: `…/github.com/{owner}/{repo}/blob/{branch}/{path}` →
///   `…/raw.githubusercontent.com/{owner}/{repo}/{branch}/{path}`
/// - tree (directory): same rewrite, then append `/SKILL.md`.
fn download_url_from_source_url(source_url: &str) -> Option<String> {
    let rest = source_url
        .strip_prefix("https://github.com/")
        .or_else(|| source_url.strip_prefix("http://github.com/"))?;

    // {owner}/{repo}/{blob|tree}/{branch}/{path...}
    let parts: Vec<&str> = rest.splitn(5, '/').collect();
    if parts.len() < 5 {
        return None;
    }
    let (owner, repo, kind, branch, path) = (parts[0], parts[1], parts[2], parts[3], parts[4]);
    if owner.is_empty() || repo.is_empty() || branch.is_empty() || path.is_empty() {
        return None;
    }

    let path = path.trim_end_matches('/');
    let raw = format!("https://raw.githubusercontent.com/{owner}/{repo}/{branch}/{path}");
    match kind {
        // blob points directly at a file; only append SKILL.md if it isn't one.
        "blob" => {
            if raw.ends_with("/SKILL.md") || raw.ends_with(".md") {
                Some(raw)
            } else {
                Some(format!("{raw}/SKILL.md"))
            }
        }
        // tree points at a directory — the skill's SKILL.md lives inside it.
        "tree" => Some(format!("{raw}/SKILL.md")),
        _ => None,
    }
}

fn download_url_from_docs_path(docs_path: &str) -> Option<String> {
    let parts: Vec<&str> = docs_path.split('/').collect();
    if parts.len() != 3 {
        return None;
    }
    let root = match parts[0] {
        "bundled" => "skills",
        "optional" => "optional-skills",
        _ => return None,
    };
    let category = parts[1];
    let prefixed_slug = parts[2];
    let skill = prefixed_slug
        .strip_prefix(&format!("{category}-"))
        .unwrap_or(prefixed_slug);
    // `category` and `skill` come from the catalog and are spliced into a URL
    // path. A reserved character (space, `#`, `?`, `%`) would change what the
    // URL names, so an entry that is not a plain path segment gets no download
    // URL and is reported as not installable rather than fetched from a
    // different path.
    if !is_safe_segment(category) || !is_safe_segment(skill) {
        tracing::debug!(
            docs_path = %docs_path,
            "[skill_registry] docsPath has a non-plain path segment; no download URL"
        );
        return None;
    }
    Some(format!(
        "https://raw.githubusercontent.com/NousResearch/hermes-agent/main/{root}/{category}/{skill}/SKILL.md"
    ))
}

/// A catalog value is spliced into a URL path, so it must be one plain segment.
pub(super) fn is_safe_segment(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

/// Raw `SKILL.md` URL for a ClawHub skill slug.
pub(super) fn clawhub_download_url(slug: &str) -> Option<String> {
    is_safe_segment(slug).then(|| format!("{CLAWHUB_SKILLS_API}/{slug}/file?path=SKILL.md"))
}

/// A skills.sh listing, `https://skills.sh/<owner>/<repo>/<skill>`.
#[derive(Debug)]
pub(super) struct SkillsShRef<'a> {
    pub(super) owner: &'a str,
    pub(super) repo: &'a str,
    pub(super) skill: &'a str,
}

impl<'a> SkillsShRef<'a> {
    pub(super) fn parse(source_url: &'a str) -> Option<Self> {
        let rest = source_url.strip_prefix("https://skills.sh/")?;
        let mut parts = rest.trim_end_matches('/').split('/');
        let (owner, repo, skill) = (parts.next()?, parts.next()?, parts.next()?);
        if parts.next().is_some() || ![owner, repo, skill].into_iter().all(is_safe_segment) {
            return None;
        }
        Some(Self { owner, repo, skill })
    }

    /// Raw URLs of the conventional skill locations, most common first.
    pub(super) fn candidate_urls(&self) -> Vec<String> {
        SKILLS_SH_BASE_DIRS
            .iter()
            .map(|base| self.raw_url(&format!("{base}{}/SKILL.md", self.skill)))
            .collect()
    }

    /// Raw URL of `path` in this repo. Each segment is percent-encoded: a path
    /// from the repo's tree listing can hold `#`, `?` or spaces, which would
    /// otherwise cut the URL short or change what it names.
    fn raw_url(&self, path: &str) -> String {
        let mut url = url::Url::parse(GITHUB_RAW).expect("GITHUB_RAW is a valid base URL");
        url.path_segments_mut()
            .expect("an https URL has path segments")
            .extend([self.owner, self.repo, "HEAD"])
            .extend(path.split('/'));
        url.into()
    }

    /// Locate this skill's `SKILL.md` in its GitHub repo.
    pub(super) async fn resolve(&self) -> Result<String, String> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(PROBE_TIMEOUT_SECS))
            .user_agent("openhuman-core")
            .build()
            .map_err(|e| format!("failed to build http client: {e}"))?;

        // Probe every conventional location at once: probed one after another,
        // each slow miss could spend the whole timeout before the next starts.
        let candidates = self.candidate_urls();
        let responses =
            futures::future::join_all(candidates.iter().map(|url| client.head(url).send())).await;
        for (url, response) in candidates.into_iter().zip(responses) {
            match response {
                Ok(resp) if resp.status().is_success() => {
                    tracing::info!(url = %url, "[skill_registry] skills.sh SKILL.md found");
                    return Ok(url);
                }
                Ok(resp) => tracing::debug!(
                    url = %url,
                    status = resp.status().as_u16(),
                    "[skill_registry] skills.sh candidate missing"
                ),
                Err(error) => tracing::debug!(
                    url = %url,
                    error = %error,
                    "[skill_registry] skills.sh candidate probe failed"
                ),
            }
        }

        // Not in a conventional directory: one recursive listing finds it anywhere.
        let repo = format!("github.com/{}/{}", self.owner, self.repo);
        let tree_url = format!(
            "{GITHUB_REPOS_API}/{}/{}/git/trees/HEAD?recursive=1",
            self.owner, self.repo
        );
        tracing::info!(repo = %repo, skill = %self.skill, "[skill_registry] listing repo tree for skills.sh skill");
        let resp = client
            .get(&tree_url)
            .send()
            .await
            .map_err(|e| format!("could not list {repo} to locate '{}': {e}", self.skill))?;
        if !resp.status().is_success() {
            return Err(format!(
                "could not list {repo} to locate '{}' (GitHub returned {})",
                self.skill,
                resp.status().as_u16()
            ));
        }
        let tree: Value = resp
            .json()
            .await
            .map_err(|e| format!("could not read the {repo} file listing: {e}"))?;
        let skill = self.skill;
        match find_skill_md_in_tree(&tree, skill) {
            Ok(path) => Ok(self.raw_url(&path)),
            Err(TreeMiss::Absent) => Err(format!(
                "'{skill}' is listed on skills.sh, but {repo} has no {skill}/SKILL.md"
            )),
            Err(TreeMiss::Truncated) => Err(format!(
                "{repo} is too large for GitHub to list in one response, so '{skill}' could not be located"
            )),
            Err(TreeMiss::Ambiguous(paths)) => Err(format!(
                "{repo} has more than one {skill}/SKILL.md ({}), and skills.sh does not say which one it lists",
                paths.join(", ")
            )),
        }
    }
}

/// Why a repo tree listing did not yield exactly one skill location.
#[derive(Debug, PartialEq)]
pub(super) enum TreeMiss {
    /// The listing is complete and has no `<skill>/SKILL.md`.
    Absent,
    /// GitHub truncated the listing and it shows at most one match, so neither
    /// absence nor uniqueness is proven.
    Truncated,
    /// Several directories are named for the skill; picking one would be a guess.
    Ambiguous(Vec<String>),
}

/// The single path of `<skill>/SKILL.md` at any depth in a GitHub recursive
/// tree listing.
pub(super) fn find_skill_md_in_tree(tree: &Value, skill: &str) -> Result<String, TreeMiss> {
    let suffix = format!("/{skill}/SKILL.md");
    let mut matches: Vec<String> = tree
        .get("tree")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|item| item.get("type").and_then(Value::as_str) == Some("blob"))
        .filter_map(|item| item.get("path").and_then(Value::as_str))
        .filter(|path| path.ends_with(&suffix) || *path == &suffix[1..])
        .map(str::to_string)
        .collect();
    // A truncated listing can omit a match: it proves neither that the skill is
    // absent nor that a lone visible match is the only one. Two visible matches
    // are ambiguous either way.
    let truncated = tree.get("truncated").and_then(Value::as_bool) == Some(true);
    if truncated && matches.len() < 2 {
        return Err(TreeMiss::Truncated);
    }
    match matches.len() {
        1 => Ok(matches.remove(0)),
        0 => Err(TreeMiss::Absent),
        _ => Err(TreeMiss::Ambiguous(matches)),
    }
}

#[cfg(test)]
#[path = "download_tests.rs"]
mod tests;
