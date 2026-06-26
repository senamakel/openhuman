//! Git history for derived wiki summary nodes.
//!
//! The repository lives at `<content_root>/wiki/.git` and intentionally tracks
//! only summary-node markdown (`summaries/**`) plus its own restrictive
//! `.gitignore`. Raw source mirrors, chunk intermediates, Obsidian defaults,
//! and future non-summary wiki artifacts are left out of history.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use git2::Oid;
use git2::{Repository, Signature};

use super::paths::WIKI_PREFIX;

static WIKI_GIT_LOCK: Mutex<()> = Mutex::new(());

const SIG_NAME: &str = "OpenHuman Memory";
const SIG_EMAIL: &str = "memory-wiki@openhuman.local";
const GITIGNORE_BODY: &str = "*\n!/.gitignore\n!/summaries/\n!/summaries/**\n";

/// Metadata for one summary node included in a wiki git commit.
#[derive(Clone, Debug)]
pub struct SummaryCommitEntry {
    pub summary_id: String,
    pub content_path: String,
    pub level: u32,
    pub child_count: usize,
    pub token_count: u32,
    pub time_range_start: DateTime<Utc>,
    pub time_range_end: DateTime<Utc>,
}

/// Metadata for one tree seal represented as a wiki git commit.
#[derive(Clone, Debug)]
pub struct SummaryCommitBatch {
    pub reason: String,
    pub tree_id: String,
    pub tree_scope: String,
    pub entries: Vec<SummaryCommitEntry>,
}

/// Ensure the wiki repository exists and has a commit containing the supplied
/// summary files. Existing non-summary tracked entries are removed from the
/// index so history stays scoped to summary nodes only.
pub fn commit_summaries(content_root: &Path, batch: &SummaryCommitBatch) -> Result<()> {
    if batch.entries.is_empty() {
        return Ok(());
    }
    let summary_repo_paths: Vec<String> = batch
        .entries
        .iter()
        .map(|entry| summary_repo_path(&entry.content_path))
        .collect::<Result<Vec<_>>>()?;
    let _guard = WIKI_GIT_LOCK.lock().expect("memory wiki git lock poisoned");

    let repo = open_prepared_repo(content_root)?;

    let mut index = repo.index().context("open wiki git index")?;
    prune_non_summary_entries(&mut index)?;
    index
        .add_path(Path::new(".gitignore"))
        .context("stage wiki .gitignore")?;
    for path in &summary_repo_paths {
        index
            .add_path(Path::new(path))
            .with_context(|| format!("stage wiki summary: {path}"))?;
    }
    index
        .write()
        .context("write wiki git index after staging summary")?;

    commit_index_if_changed(&repo, batch)
}

/// Add a timestamped lightweight git tag that represents a reader's high-water
/// mark, and move a stable `latest` alias for quick lookup.
///
/// This writes `refs/tags/read/<hex(pointer_id)>/<YYYYMMDDTHHMMSS.nnnnnnnnnZ>`
/// to `target_commit`, or to wiki `HEAD` when `target_commit` is `None`, and
/// also updates `refs/tags/read/<hex(pointer_id)>/latest`. Tags update read
/// state without creating another history commit.
pub fn set_read_pointer_tag(
    content_root: &Path,
    pointer_id: &str,
    target_commit: Option<&str>,
) -> Result<String> {
    let _guard = WIKI_GIT_LOCK.lock().expect("memory wiki git lock poisoned");
    let repo = open_prepared_repo(content_root)?;
    let oid = match target_commit {
        Some(commit) => {
            Oid::from_str(commit).with_context(|| format!("bad commit id: {commit}"))?
        }
        None => repo.head()?.peel_to_commit()?.id(),
    };
    let tag_ref = read_pointer_timestamp_ref(pointer_id, Utc::now());
    repo.reference(&tag_ref, oid, true, "advance memory wiki read pointer")
        .with_context(|| format!("set wiki read pointer tag: {tag_ref}"))?;
    let latest_ref = read_pointer_latest_ref(pointer_id);
    repo.reference(
        &latest_ref,
        oid,
        true,
        "advance latest memory wiki read pointer",
    )
    .with_context(|| format!("set latest wiki read pointer tag: {latest_ref}"))?;
    log::debug!(
        "[content_store::wiki_git] advanced read pointer tags {} latest={} -> {}",
        tag_ref,
        latest_ref,
        oid
    );
    Ok(oid.to_string())
}

/// Return the commit id a read-pointer tag currently references.
pub fn get_read_pointer_tag(content_root: &Path, pointer_id: &str) -> Result<Option<String>> {
    let _guard = WIKI_GIT_LOCK.lock().expect("memory wiki git lock poisoned");
    let wiki_root = content_root.join(WIKI_PREFIX);
    let Ok(repo) = Repository::open(&wiki_root) else {
        return Ok(None);
    };
    let tag_ref = read_pointer_latest_ref(pointer_id);
    let target = match repo.find_reference(&tag_ref) {
        Ok(reference) => Ok(reference.target().map(|oid| oid.to_string())),
        Err(_) => Ok(None),
    };
    target
}

fn open_prepared_repo(content_root: &Path) -> Result<Repository> {
    let wiki_root = content_root.join(WIKI_PREFIX);
    std::fs::create_dir_all(&wiki_root)
        .with_context(|| format!("create wiki git root: {}", wiki_root.display()))?;

    let repo = open_or_init_repo(&wiki_root)?;
    ensure_gitignore(&wiki_root)?;
    Ok(repo)
}

fn open_or_init_repo(wiki_root: &Path) -> Result<Repository> {
    match Repository::open(wiki_root) {
        Ok(repo) => Ok(repo),
        Err(_) => {
            log::debug!(
                "[content_store::wiki_git] initialising summary wiki git repo at {}",
                wiki_root.display()
            );
            Repository::init(wiki_root)
                .with_context(|| format!("init wiki git repo: {}", wiki_root.display()))
        }
    }
}

fn ensure_gitignore(wiki_root: &Path) -> Result<()> {
    let path = wiki_root.join(".gitignore");
    match std::fs::read_to_string(&path) {
        Ok(existing) if existing == GITIGNORE_BODY => Ok(()),
        _ => {
            std::fs::write(&path, GITIGNORE_BODY)
                .with_context(|| format!("write wiki gitignore: {}", path.display()))?;
            log::debug!(
                "[content_store::wiki_git] wrote summary-only .gitignore at {}",
                path.display()
            );
            Ok(())
        }
    }
}

fn prune_non_summary_entries(index: &mut git2::Index) -> Result<()> {
    let to_remove: Vec<PathBuf> = index
        .iter()
        .filter_map(|entry| {
            let path = std::str::from_utf8(&entry.path).ok()?;
            if is_tracked_wiki_path(path) {
                None
            } else {
                Some(PathBuf::from(path))
            }
        })
        .collect();

    for path in to_remove {
        index
            .remove_path(&path)
            .with_context(|| format!("remove non-summary wiki git entry: {}", path.display()))?;
    }
    Ok(())
}

fn is_tracked_wiki_path(path: &str) -> bool {
    path == ".gitignore" || path.starts_with("summaries/")
}

fn commit_index_if_changed(repo: &Repository, batch: &SummaryCommitBatch) -> Result<()> {
    let tree_oid = repo.index()?.write_tree()?;
    let tree = repo.find_tree(tree_oid)?;

    let parent_commit = match repo.head() {
        Ok(head) => Some(head.peel_to_commit()?),
        Err(_) => None,
    };

    if let Some(parent) = &parent_commit {
        if parent.tree_id() == tree_oid {
            log::debug!(
                "[content_store::wiki_git] summary wiki git clean after staging tree_id={} entries={}",
                batch.tree_id,
                batch.entries.len()
            );
            return Ok(());
        }
    }

    let sig = Signature::now(SIG_NAME, SIG_EMAIL).context("build wiki git signature")?;
    let message = build_commit_message(batch);
    let parents: Vec<&git2::Commit> = parent_commit.iter().collect();
    let commit_oid = repo
        .commit(Some("HEAD"), &sig, &sig, &message, &tree, &parents)
        .context("commit wiki summary update")?;

    log::debug!(
        "[content_store::wiki_git] committed summary wiki update commit={} tree_id={} entries={}",
        commit_oid,
        batch.tree_id,
        batch.entries.len()
    );
    Ok(())
}

fn build_commit_message(batch: &SummaryCommitBatch) -> String {
    let mut min_level = u32::MAX;
    let mut max_level = 0;
    let mut child_count = 0usize;
    let mut token_count = 0u32;
    let mut start: Option<DateTime<Utc>> = None;
    let mut end: Option<DateTime<Utc>> = None;

    for entry in &batch.entries {
        min_level = min_level.min(entry.level);
        max_level = max_level.max(entry.level);
        child_count = child_count.saturating_add(entry.child_count);
        token_count = token_count.saturating_add(entry.token_count);
        start = Some(start.map_or(entry.time_range_start, |s| s.min(entry.time_range_start)));
        end = Some(end.map_or(entry.time_range_end, |e| e.max(entry.time_range_end)));
    }

    let level_label = if min_level == max_level {
        format!("L{min_level}")
    } else {
        format!("L{min_level}-L{max_level}")
    };
    let title = format!(
        "Seal memory tree {} {} summaries",
        batch.tree_scope, level_label
    );

    let mut msg = String::new();
    msg.push_str(&title);
    msg.push_str("\n\n");
    msg.push_str(&format!("Reason: {}\n", batch.reason));
    msg.push_str(&format!("Tree-Id: {}\n", batch.tree_id));
    msg.push_str(&format!("Tree-Scope: {}\n", batch.tree_scope));
    msg.push_str(&format!("Summary-Count: {}\n", batch.entries.len()));
    msg.push_str(&format!("Level-Range: {level_label}\n"));
    msg.push_str(&format!("Child-Count: {child_count}\n"));
    msg.push_str(&format!("Token-Count: {token_count}\n"));
    if let (Some(start), Some(end)) = (start, end) {
        msg.push_str(&format!("Time-Range-Start: {}\n", start.to_rfc3339()));
        msg.push_str(&format!("Time-Range-End: {}\n", end.to_rfc3339()));
    }
    msg.push_str("\nSummaries:\n");
    for entry in &batch.entries {
        msg.push_str(&format!(
            "- {} L{} children={} tokens={} path={}\n",
            entry.summary_id, entry.level, entry.child_count, entry.token_count, entry.content_path
        ));
    }
    msg
}

fn summary_repo_path(summary_content_path: &str) -> Result<String> {
    let prefix = format!("{WIKI_PREFIX}/");
    let Some(repo_path) = summary_content_path.strip_prefix(&prefix) else {
        anyhow::bail!(
            "summary content path must live under {WIKI_PREFIX}/: {summary_content_path}"
        );
    };
    if !repo_path.starts_with("summaries/") {
        anyhow::bail!("wiki git only tracks summary nodes: {summary_content_path}");
    }
    Ok(repo_path.to_string())
}

fn read_pointer_latest_ref(pointer_id: &str) -> String {
    format!(
        "refs/tags/read/{}/latest",
        hex::encode(pointer_id.as_bytes())
    )
}

fn read_pointer_timestamp_ref(pointer_id: &str, timestamp: DateTime<Utc>) -> String {
    format!(
        "refs/tags/read/{}/{}",
        hex::encode(pointer_id.as_bytes()),
        timestamp.format("%Y%m%dT%H%M%S%.9fZ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use git2::IndexAddOption;
    use tempfile::TempDir;

    #[test]
    fn commit_summary_initializes_repo_and_tracks_only_summaries() {
        let dir = TempDir::new().unwrap();
        let wiki = dir.path().join("wiki");
        let summary = wiki.join("summaries/source-slack/L1/summary-1.md");
        let raw = wiki.join("raw/should-not-track.md");
        let note = wiki.join("notes/also-ignored.md");
        std::fs::create_dir_all(summary.parent().unwrap()).unwrap();
        std::fs::create_dir_all(raw.parent().unwrap()).unwrap();
        std::fs::create_dir_all(note.parent().unwrap()).unwrap();
        std::fs::write(&summary, "---\nkind: summary\n---\nbody").unwrap();
        std::fs::write(&raw, "raw").unwrap();
        std::fs::write(&note, "note").unwrap();

        commit_summaries(
            dir.path(),
            &batch(
                "queued_seal",
                vec![entry(
                    "summary-1",
                    "wiki/summaries/source-slack/L1/summary-1.md",
                )],
            ),
        )
        .unwrap();

        let repo = Repository::open(&wiki).unwrap();
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        let tree = head.tree().unwrap();
        assert!(tree.get_path(Path::new(".gitignore")).is_ok());
        assert!(tree
            .get_path(Path::new("summaries/source-slack/L1/summary-1.md"))
            .is_ok());
        assert!(tree.get_path(Path::new("raw/should-not-track.md")).is_err());
        assert!(tree.get_path(Path::new("notes/also-ignored.md")).is_err());
    }

    #[test]
    fn commit_summary_prunes_existing_non_summary_tracked_entries() {
        let dir = TempDir::new().unwrap();
        let wiki = dir.path().join("wiki");
        std::fs::create_dir_all(wiki.join("raw")).unwrap();
        std::fs::create_dir_all(wiki.join("summaries/source/L1")).unwrap();
        std::fs::write(wiki.join("raw/old.md"), "old raw").unwrap();
        std::fs::write(wiki.join("summaries/source/L1/new.md"), "new summary").unwrap();

        let repo = Repository::init(&wiki).unwrap();
        let mut index = repo.index().unwrap();
        index
            .add_all(["*"].iter(), IndexAddOption::DEFAULT, None)
            .unwrap();
        index.write().unwrap();
        let tree_oid = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_oid).unwrap();
        let sig = Signature::now(SIG_NAME, SIG_EMAIL).unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "old mixed commit", &tree, &[])
            .unwrap();

        commit_summaries(
            dir.path(),
            &batch(
                "queued_seal",
                vec![entry("new", "wiki/summaries/source/L1/new.md")],
            ),
        )
        .unwrap();

        let head = repo.head().unwrap().peel_to_commit().unwrap();
        let tree = head.tree().unwrap();
        assert!(tree
            .get_path(Path::new("summaries/source/L1/new.md"))
            .is_ok());
        assert!(tree.get_path(Path::new("raw/old.md")).is_err());
    }

    #[test]
    fn commit_summary_rejects_non_summary_paths() {
        let dir = TempDir::new().unwrap();
        let err = commit_summaries(
            dir.path(),
            &batch("bad", vec![entry("bad", "wiki/notes/one.md")]),
        )
        .unwrap_err();
        assert!(err.to_string().contains("only tracks summary nodes"));
    }

    #[test]
    fn commit_message_describes_seal_metadata() {
        let dir = TempDir::new().unwrap();
        let wiki = dir.path().join("wiki");
        let summary = wiki.join("summaries/source/L2/summary-2.md");
        std::fs::create_dir_all(summary.parent().unwrap()).unwrap();
        std::fs::write(&summary, "---\nkind: summary\n---\nbody").unwrap();

        commit_summaries(
            dir.path(),
            &batch(
                "sync_cascade",
                vec![SummaryCommitEntry {
                    summary_id: "summary-2".to_string(),
                    content_path: "wiki/summaries/source/L2/summary-2.md".to_string(),
                    level: 2,
                    child_count: 7,
                    token_count: 123,
                    time_range_start: ts(1_700_000_000_000),
                    time_range_end: ts(1_700_003_600_000),
                }],
            ),
        )
        .unwrap();

        let repo = Repository::open(&wiki).unwrap();
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        let msg = head.message().unwrap();
        assert!(msg.contains("Seal memory tree slack:#eng L2 summaries"));
        assert!(msg.contains("Reason: sync_cascade"));
        assert!(msg.contains("Summary-Count: 1"));
        assert!(msg.contains("Child-Count: 7"));
        assert!(msg.contains("Token-Count: 123"));
        assert!(msg.contains("summary-2 L2 children=7 tokens=123"));
    }

    #[test]
    fn read_pointer_tags_are_timestamped_and_move_latest_without_new_commit() {
        let dir = TempDir::new().unwrap();
        let wiki = dir.path().join("wiki");
        let summary = wiki.join("summaries/source/L1/summary-1.md");
        std::fs::create_dir_all(summary.parent().unwrap()).unwrap();
        std::fs::write(&summary, "---\nkind: summary\n---\nbody").unwrap();
        commit_summaries(
            dir.path(),
            &batch(
                "queued_seal",
                vec![entry("summary-1", "wiki/summaries/source/L1/summary-1.md")],
            ),
        )
        .unwrap();

        let repo = Repository::open(&wiki).unwrap();
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        let head_id = head.id().to_string();

        let tagged = set_read_pointer_tag(dir.path(), "agent:default", None).unwrap();
        assert_eq!(tagged, head_id);
        assert_eq!(
            get_read_pointer_tag(dir.path(), "agent:default")
                .unwrap()
                .as_deref(),
            Some(head_id.as_str())
        );
        let tag_prefix = format!(
            "refs/tags/read/{}/",
            hex::encode("agent:default".as_bytes())
        );
        let tags = repo.references().unwrap().fold(Vec::new(), |mut acc, r| {
            let r = r.unwrap();
            let name = r.name().unwrap();
            if name.starts_with(&tag_prefix) {
                acc.push(name.to_string());
            }
            acc
        });
        assert!(
            tags.iter().any(|name| name.ends_with("/latest")),
            "latest read pointer tag should be present: {tags:?}"
        );
        assert!(
            tags.iter().any(|name| {
                let suffix = name.strip_prefix(&tag_prefix).unwrap_or_default();
                suffix.len() == "20260626T045537.123456789Z".len()
                    && suffix.ends_with('Z')
                    && suffix.contains('T')
            }),
            "timestamped read pointer tag should be present: {tags:?}"
        );
        let mut walk = repo.revwalk().unwrap();
        walk.push_head().unwrap();
        assert_eq!(
            walk.count(),
            1,
            "moving the read pointer must not create commits"
        );
    }

    fn batch(reason: &str, entries: Vec<SummaryCommitEntry>) -> SummaryCommitBatch {
        SummaryCommitBatch {
            reason: reason.to_string(),
            tree_id: "tree-1".to_string(),
            tree_scope: "slack:#eng".to_string(),
            entries,
        }
    }

    fn entry(summary_id: &str, content_path: &str) -> SummaryCommitEntry {
        SummaryCommitEntry {
            summary_id: summary_id.to_string(),
            content_path: content_path.to_string(),
            level: 1,
            child_count: 2,
            token_count: 10,
            time_range_start: ts(1_700_000_000_000),
            time_range_end: ts(1_700_000_001_000),
        }
    }

    fn ts(ms: i64) -> DateTime<Utc> {
        DateTime::<Utc>::from_timestamp_millis(ms).unwrap()
    }
}
