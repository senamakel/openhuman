//! Working-directory property providers surfaced into the agent's context at a
//! workflow phase (e.g. `context: [git]`).
//!
//! Each provider reads a cheap, read-only snapshot of the working directory.
//! Git is the only provider in v1; the dispatch is keyed on provider name so
//! more (project type, package manager, …) can slot in later.

use std::path::Path;
use std::time::Duration;

use super::types::CONTEXT_GIT;

/// Hard cap on how long any single provider command may run.
const PROVIDER_TIMEOUT: Duration = Duration::from_secs(10);

/// Build the working-directory context block for the requested `providers`.
/// Returns `None` when no provider produced output.
pub async fn working_dir_context(dir: &Path, providers: &[String]) -> Option<String> {
    let mut sections: Vec<String> = Vec::new();
    for provider in providers {
        match provider.trim().to_ascii_lowercase().as_str() {
            CONTEXT_GIT => {
                if let Some(block) = git_context(dir).await {
                    sections.push(block);
                }
            }
            other => {
                tracing::debug!(provider = %other, "[workflows][workdir] unknown context provider; skipping");
            }
        }
    }
    if sections.is_empty() {
        return None;
    }
    Some(format!(
        "**Working directory:** `{}`\n\n{}",
        dir.display(),
        sections.join("\n")
    ))
}

/// Collect a compact git snapshot: branch, dirty/clean, and recent commits.
async fn git_context(dir: &Path) -> Option<String> {
    let branch = run_git(dir, &["rev-parse", "--abbrev-ref", "HEAD"]).await;
    let branch = match branch {
        Some(b) if !b.trim().is_empty() => b.trim().to_string(),
        // Not a git repo (or git missing) — surface nothing rather than noise.
        _ => {
            tracing::debug!(dir = %dir.display(), "[workflows][workdir] not a git repo; skipping git context");
            return None;
        }
    };

    let status = run_git(dir, &["status", "--porcelain"])
        .await
        .unwrap_or_default();
    let dirty_count = status.lines().filter(|l| !l.trim().is_empty()).count();
    let log = run_git(dir, &["log", "-3", "--oneline"])
        .await
        .unwrap_or_default();

    let mut out = String::from("**Git:**\n");
    out.push_str(&format!("- branch: `{branch}`\n"));
    if dirty_count == 0 {
        out.push_str("- status: clean\n");
    } else {
        out.push_str(&format!("- status: {dirty_count} uncommitted change(s)\n"));
    }
    if !log.trim().is_empty() {
        out.push_str("- recent commits:\n");
        for line in log.lines().take(3) {
            out.push_str(&format!("  - {}\n", line.trim()));
        }
    }
    Some(out)
}

/// Run a read-only `git` command in `dir`, returning trimmed stdout on success.
async fn run_git(dir: &Path, args: &[&str]) -> Option<String> {
    let mut cmd = tokio::process::Command::new("git");
    cmd.args(args).current_dir(dir);
    let fut = cmd.output();
    let output = match tokio::time::timeout(PROVIDER_TIMEOUT, fut).await {
        Ok(Ok(out)) => out,
        Ok(Err(e)) => {
            tracing::debug!(error = %e, args = ?args, "[workflows][workdir] git invocation failed");
            return None;
        }
        Err(_) => {
            tracing::debug!(args = ?args, "[workflows][workdir] git invocation timed out");
            return None;
        }
    };
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).to_string())
}
