//! Working-directory context providers.
//!
//! Builds a compact, agent-readable block describing properties of the current
//! working directory (git branch / status / recent log). Extensible: future
//! providers (project type, language, etc.) plug into [`working_dir_context`].

use std::path::Path;

use crate::openhuman::tools::r#impl::system::workspace_state::run_git;

/// Build the working-dir context block for the requested `providers`. Returns
/// an empty string when no providers are requested.
pub fn working_dir_context(dir: &Path, providers: &[String]) -> String {
    if providers.is_empty() {
        return String::new();
    }
    let mut out = String::from("### Working directory context\n");
    out.push_str(&format!("- path: {}\n", dir.display()));
    for provider in providers {
        match provider.as_str() {
            "git" => out.push_str(&git_block(dir)),
            other => out.push_str(&format!("- unknown context provider: {other}\n")),
        }
    }
    out
}

/// Render the git context: branch, dirty flag, porcelain status, recent log.
fn git_block(dir: &Path) -> String {
    let mut b = String::new();
    match run_git(dir, &["rev-parse", "--abbrev-ref", "HEAD"]) {
        Ok(branch) => b.push_str(&format!("- git branch: {}\n", branch.trim())),
        Err(_) => {
            b.push_str("- git: not a git repository\n");
            return b;
        }
    }

    let status = run_git(dir, &["status", "--porcelain"]).unwrap_or_default();
    let dirty = !status.trim().is_empty();
    b.push_str(&format!("- git dirty: {dirty}\n"));
    if dirty {
        b.push_str("- git status:\n");
        for line in status.lines().take(10) {
            b.push_str(&format!("    {line}\n"));
        }
    }

    if let Ok(log) = run_git(dir, &["log", "--oneline", "-5"]) {
        if !log.trim().is_empty() {
            b.push_str("- recent commits:\n");
            for line in log.lines().take(5) {
                b.push_str(&format!("    {line}\n"));
            }
        }
    }
    b
}

#[cfg(test)]
#[path = "workdir_tests.rs"]
mod tests;
