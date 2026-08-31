//! Skills that ship **inside the binary**.
//!
//! A bundled skill is an ordinary SKILL.md bundle — frontmatter, a body, and
//! `references/` files — whose bytes are `include_str!`'d at compile time from
//! a directory in this repository. At boot it is written into the workspace
//! under [`builtin_root`] and from then on it is discovered, described, read
//! and run by exactly the same code paths as a skill the user installed. There
//! is no second reader, no virtual filesystem, and no `location: None` case for
//! downstream code to handle.
//!
//! # Why materialise instead of serving from memory
//!
//! Every consumer downstream of discovery resolves a real path:
//! `read_workflow_resource` canonicalises against the bundle root and refuses
//! symlinks, `run_skill` hands the worker a directory, and the UI shows the
//! user where the bundle lives. Serving embedded bytes instead would mean an
//! `Option<PathBuf>` branch in each of those, and the security properties of
//! the resource reader — traversal and symlink rejection — would have to be
//! restated for the in-memory case rather than inherited. Writing the files out
//! once per version is cheaper than that, and it also makes a bundled skill
//! something the user can read on disk.
//!
//! # What this is NOT
//!
//! It is not an extension point. The table below is a `const` compiled into the
//! binary, exactly like [`crate::openhuman::modules::registry`], and for the
//! same reason: a table that config or RPC could add entries to would let a
//! remote party place instructions in front of the model. A skill the user
//! wants comes from `skill_registry_install`, which has its own review path.
//!
//! # Precedence
//!
//! Builtin is the **lowest** scope ([`WorkflowScope::Builtin`]). A user or
//! project skill with the same name shadows it, so shipping a bundle can never
//! take a name away from someone who already used it.

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

/// One file inside a bundled skill, relative to the bundle directory.
#[derive(Debug, Clone, Copy)]
pub struct BundledFile {
    /// Slash-separated path relative to the bundle root (`WORKFLOW.md`,
    /// `references/expressions.md`). Validated by
    /// [`BundledSkill::validate`] — see the test module.
    pub path: &'static str,
    pub contents: &'static str,
}

/// A skill compiled into the binary.
#[derive(Debug, Clone, Copy)]
pub struct BundledSkill {
    /// The on-disk directory name, and the id the model names in `run_skill` /
    /// `describe_workflow`.
    pub dir_name: &'static str,
    pub files: &'static [BundledFile],
}

impl BundledSkill {
    /// A digest over the bundle's full contents, used to decide whether the
    /// materialised copy on disk is current.
    ///
    /// Both the path and the contents of every file are hashed, each
    /// length-prefixed, so that renaming a file or moving a byte between two
    /// files changes the digest. Without the length prefix the concatenation
    /// `("ab", "c")` and `("a", "bc")` would hash identically.
    pub fn digest(&self) -> String {
        let mut hasher = Sha256::new();
        for file in self.files {
            hasher.update((file.path.len() as u64).to_le_bytes());
            hasher.update(file.path.as_bytes());
            hasher.update((file.contents.len() as u64).to_le_bytes());
            hasher.update(file.contents.as_bytes());
        }
        format!("{:x}", hasher.finalize())
    }

    /// Reject a bundle this host will not write out.
    ///
    /// These are compile-time constants from this repository, so none of this
    /// can fire in a shipped build — which is exactly why it is checked by a
    /// test rather than trusted. The cost of being wrong is writing outside the
    /// workspace, and "our own constants are fine" is the assumption that makes
    /// that mistake survivable for one refactor too long.
    pub fn validate(&self) -> Result<(), String> {
        if self.dir_name.is_empty()
            || self.dir_name.starts_with('.')
            || self.dir_name.contains('/')
            || self.dir_name.contains('\\')
        {
            return Err(format!("invalid bundled skill dir_name `{}`", self.dir_name));
        }
        if self.files.is_empty() {
            return Err(format!("bundled skill `{}` has no files", self.dir_name));
        }
        let has_manifest = self
            .files
            .iter()
            .any(|f| f.path == super::ops_types::WORKFLOW_MD || f.path == super::ops_types::SKILL_MD);
        if !has_manifest {
            return Err(format!(
                "bundled skill `{}` has no WORKFLOW.md or SKILL.md; discovery would skip it",
                self.dir_name
            ));
        }
        for file in self.files {
            validate_relative_path(self.dir_name, file.path)?;
        }
        Ok(())
    }
}

fn validate_relative_path(dir_name: &str, path: &str) -> Result<(), String> {
    if path.is_empty() {
        return Err(format!("bundled skill `{dir_name}` has an empty file path"));
    }
    if path.starts_with('/') || path.starts_with('\\') || path.contains(':') {
        return Err(format!(
            "bundled skill `{dir_name}` file `{path}` is not workspace-relative"
        ));
    }
    for component in path.split('/') {
        if component.is_empty() || component == "." || component == ".." {
            return Err(format!(
                "bundled skill `{dir_name}` file `{path}` has a traversal component"
            ));
        }
        if component.starts_with('.') {
            return Err(format!(
                "bundled skill `{dir_name}` file `{path}` has a dotfile component; \
                 discovery skips those"
            ));
        }
    }
    Ok(())
}

/// The compiled-in table.
///
/// Gated per entry, not as a whole: `flow-authoring` teaches the flows node
/// vocabulary, and in a build without the `flows` feature there is nothing for
/// it to author. Shipping it anyway would put a manual for absent tools in
/// front of the model — the same failure the `Off` tool mode exists to avoid.
pub const BUNDLED: &[BundledSkill] = &[
    #[cfg(feature = "flows")]
    crate::openhuman::flows::skills::FLOW_AUTHORING,
];

/// Where materialised bundles live.
///
/// Its own root rather than a subdirectory of `.openhuman/skills/`, because the
/// contents are ours to overwrite: [`install`] deletes and rewrites a bundle
/// whose digest moved, and doing that inside a directory the user also writes
/// to would eventually delete something of theirs.
pub fn builtin_root(workspace_dir: &Path) -> PathBuf {
    workspace_dir.join(".openhuman").join("builtin-skills")
}

/// Name of the digest sidecar written beside each materialised bundle.
///
/// A dotfile so `scan_root` skips it (it skips `.`-prefixed entries) and it is
/// never mistaken for a skill resource.
const DIGEST_FILE: &str = ".digest";

/// What [`install`] did, for the boot log.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct InstallReport {
    pub written: Vec<String>,
    pub unchanged: Vec<String>,
    pub failed: Vec<(String, String)>,
}

/// Materialise every bundled skill into `workspace_dir`, skipping those whose
/// on-disk digest already matches.
///
/// Never returns `Err`: a workspace that cannot take a builtin skill should
/// boot without it rather than not boot. Failures land in
/// [`InstallReport::failed`] and are logged.
pub fn install(workspace_dir: &Path) -> InstallReport {
    let root = builtin_root(workspace_dir);
    let mut report = InstallReport::default();

    for skill in BUNDLED {
        let name = skill.dir_name.to_string();
        match install_one(&root, skill) {
            Ok(true) => {
                tracing::info!(skill = %name, "[skills][bundled] wrote builtin skill");
                report.written.push(name);
            }
            Ok(false) => {
                tracing::debug!(skill = %name, "[skills][bundled] builtin skill already current");
                report.unchanged.push(name);
            }
            Err(err) => {
                tracing::warn!(
                    skill = %name,
                    error = %err,
                    "[skills][bundled] failed to install builtin skill; it will be absent this run"
                );
                report.failed.push((name, err));
            }
        }
    }

    tracing::debug!(
        root = %root.display(),
        written = report.written.len(),
        unchanged = report.unchanged.len(),
        failed = report.failed.len(),
        "[skills][bundled] install:exit"
    );
    report
}

/// Returns `Ok(true)` when the bundle was (re)written, `Ok(false)` when the
/// on-disk copy was already current.
fn install_one(root: &Path, skill: &BundledSkill) -> Result<bool, String> {
    skill.validate()?;

    let dir = root.join(skill.dir_name);
    let digest = skill.digest();
    let digest_path = dir.join(DIGEST_FILE);

    if std::fs::read_to_string(&digest_path).is_ok_and(|found| found.trim() == digest) {
        return Ok(false);
    }

    // Remove rather than overwrite: a previous version may have shipped a file
    // this one does not, and leaving it behind would let a stale reference doc
    // keep answering `read_workflow_resource` calls forever.
    if dir.exists() {
        std::fs::remove_dir_all(&dir)
            .map_err(|e| format!("failed to clear {}: {e}", dir.display()))?;
    }

    for file in skill.files {
        let target = dir.join(file.path);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("failed to create {}: {e}", parent.display()))?;
        }
        std::fs::write(&target, file.contents)
            .map_err(|e| format!("failed to write {}: {e}", target.display()))?;
    }

    // Written last, so an interrupted install leaves no digest and the next
    // boot rewrites the bundle rather than trusting a half-written one.
    std::fs::write(&digest_path, &digest)
        .map_err(|e| format!("failed to write {}: {e}", digest_path.display()))?;

    Ok(true)
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod mod_tests;
