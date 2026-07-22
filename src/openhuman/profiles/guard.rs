//! Active-profile identity plumbing + the cross-profile write guard.
//!
//! Two concerns, both keyed off *which* profile a turn runs under:
//!
//! 1. **Identity plumbing (1a).** When a dedicated-workspace profile is active,
//!    its id is carried to the tool layer inside the
//!    [`WorkspaceDescriptor`](tinyagents::harness::workspace::WorkspaceDescriptor)'s
//!    `policy_id` field as `openhuman.profile:<id>`. [`workspace_policy_id`] and
//!    [`profile_id_from_policy_id`] are the single encode/decode pair so the
//!    session builder and the tool gates can never drift on the wire format.
//!
//! 2. **Cross-profile write guard (1b).** A hermes `file_safety` equivalent:
//!    while a turn runs under a dedicated-workspace profile `P`, any tool
//!    write/command whose resolved target lands in a *sibling* profile's
//!    workspace `<action_dir>/profiles/<Q>/` (Q != P) is blocked. See
//!    [`classify_cross_profile_target`] (file tools) and
//!    [`scan_command_for_cross_profile`] (shell). The guard only ever
//!    **tightens**: with no active profile the classifier is never consulted
//!    and behaviour is byte-identical to today.

use std::path::{Component, Path, PathBuf};

/// Wire prefix for the per-profile `WorkspaceDescriptor::policy_id`. The suffix
/// is the profile id. Kept private-behind-helpers so the encode/decode pair is
/// the only way this string is produced or parsed.
const PROFILE_POLICY_ID_PREFIX: &str = "openhuman.profile:";

/// Encode a profile id as the `WorkspaceDescriptor::policy_id` the session
/// builder stamps onto a dedicated-workspace descriptor (`openhuman.profile:<id>`).
///
/// Paired with [`profile_id_from_policy_id`]; the two are the sole owners of the
/// wire format so the encode and decode sites can never disagree.
pub fn workspace_policy_id(profile_id: &str) -> String {
    format!("{PROFILE_POLICY_ID_PREFIX}{profile_id}")
}

/// Decode the active profile id from a `WorkspaceDescriptor::policy_id`.
///
/// Returns `Some(id)` only for the `openhuman.profile:<id>` shape
/// [`workspace_policy_id`] produces (and only when `<id>` is non-empty); every
/// other policy_id — the worktree-isolation ids, test ids, or an empty string —
/// yields `None`, so a non-profile session reads as "no active profile" and the
/// tool gates stay on their shared-path behaviour.
pub fn profile_id_from_policy_id(policy_id: &str) -> Option<&str> {
    policy_id
        .strip_prefix(PROFILE_POLICY_ID_PREFIX)
        .filter(|id| !id.is_empty())
}

/// Outcome of classifying a resolved write/command target against the active
/// profile's isolation boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CrossProfileDecision {
    /// The target is not inside a *sibling* profile's workspace — permitted (the
    /// active profile's own dir, the shared `action_dir`, or anywhere outside
    /// `<action_dir>/profiles/` entirely).
    Allow,
    /// The target lands inside `<action_dir>/profiles/<other_id>/` with
    /// `other_id != active_profile` — blocked.
    Block {
        /// The sibling profile whose workspace the target tried to reach.
        other_id: String,
    },
}

/// Classify a resolved target path against the active profile's cross-profile
/// isolation boundary (the hermes `classify_cross_profile_target` analogue).
///
/// `action_dir` is the agent's **broad** action root; sibling profile
/// workspaces live under `<action_dir>/profiles/<id>/`. `active_profile` is the
/// id of the profile the turn runs under. `target` is the write/command target
/// — ideally already resolved to an absolute path, but relative inputs are
/// joined onto `action_dir` first so the classifier is robust to either.
///
/// Returns [`CrossProfileDecision::Block`] iff the canonicalized `target` is
/// inside `<action_dir>/profiles/<Q>/` for some `Q != active_profile`, otherwise
/// [`CrossProfileDecision::Allow`].
///
/// **Symlink safety.** The comparison is done on canonicalized paths: the
/// profiles root and the target's deepest *existing* ancestor are both resolved
/// through the filesystem, so a symlink inside profile `P` pointing at profile
/// `Q`'s dir still classifies as a cross-profile target. A not-yet-existing
/// target (a fresh write) canonicalizes its nearest existing ancestor and
/// re-appends the missing tail, matching the `validate_parent_path` strategy.
pub fn classify_cross_profile_target(
    action_dir: &Path,
    active_profile: &str,
    target: &Path,
) -> CrossProfileDecision {
    let profiles_root = action_dir.join("profiles");
    // Resolve the profiles root through the filesystem so a symlinked
    // action_dir (macOS `/tmp` -> `/private/tmp`, sandbox bind-mounts) compares
    // against the same canonical prefix the target resolves to.
    let canon_profiles_root = canonicalize_best_effort(&profiles_root);

    let absolute_target = if target.is_absolute() {
        target.to_path_buf()
    } else {
        action_dir.join(target)
    };
    let canon_target = canonicalize_deepest_existing(&absolute_target);

    let Ok(relative) = canon_target.strip_prefix(&canon_profiles_root) else {
        return CrossProfileDecision::Allow;
    };
    // First component under `profiles/` is the owning profile id.
    let Some(Component::Normal(owner)) = relative.components().next() else {
        return CrossProfileDecision::Allow;
    };
    let owner = owner.to_string_lossy();
    if owner == active_profile {
        CrossProfileDecision::Allow
    } else {
        CrossProfileDecision::Block {
            other_id: owner.into_owned(),
        }
    }
}

/// Best-effort scan of a shell `command` for a token that targets a sibling
/// profile's workspace, given the command's working directory `cwd` (the active
/// profile's own dir).
///
/// Shell commands do not funnel through the per-path `validate_path` gate that
/// file tools use, so this is the shell-side call site of the same guard. It
/// splits the command on whitespace and redirect operators, keeps only
/// path-shaped tokens (those containing a path separator or a leading `~`),
/// resolves each against `cwd`, and classifies it via
/// [`classify_cross_profile_target`]. Returns the first sibling profile id it
/// would write into, or `None` when the command stays clear of other profiles.
///
/// This is deliberately a *containment* backstop layered on top of the cwd —
/// which is already rooted at the profile's own workspace — rather than a full
/// shell parser. It reliably catches the realistic escape vectors (an absolute
/// path into `profiles/<Q>` or a `../<Q>/…` traversal); the airtight guarantee
/// for file mutations lives in the file-tool `validate_path` call site.
pub fn scan_command_for_cross_profile(
    command: &str,
    cwd: &Path,
    action_dir: &Path,
    active_profile: &str,
) -> Option<String> {
    for raw in command.split(|c: char| c.is_whitespace() || matches!(c, '>' | '<' | '|' | ';')) {
        let token = raw.trim_matches(|c| matches!(c, '"' | '\'' | '(' | ')' | '`'));
        if token.is_empty() {
            continue;
        }
        // Only path-shaped tokens can reach another directory: they contain a
        // separator or start with `~`. A bare word (`ls`, `bob`) resolves under
        // cwd (the profile's own dir) and is never a sibling.
        let path_shaped = token.contains('/') || token.contains('\\') || token.starts_with('~');
        if !path_shaped {
            continue;
        }
        let expanded = crate::openhuman::config::expand_tilde(token);
        let candidate = Path::new(&expanded);
        let absolute = if candidate.is_absolute() {
            candidate.to_path_buf()
        } else {
            cwd.join(candidate)
        };
        if let CrossProfileDecision::Block { other_id } =
            classify_cross_profile_target(action_dir, active_profile, &absolute)
        {
            return Some(other_id);
        }
    }
    None
}

/// Canonicalize `path`, falling back to the raw path when it does not exist.
fn canonicalize_best_effort(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

/// Canonicalize `path` when it exists; otherwise canonicalize its deepest
/// existing ancestor and re-append the missing tail. Mirrors
/// `SecurityPolicy::validate_parent_path`'s symlink-safe resolution so a fresh
/// (not-yet-created) write target is still classified against the real
/// filesystem layout.
fn canonicalize_deepest_existing(path: &Path) -> PathBuf {
    if let Ok(canonical) = path.canonicalize() {
        return canonical;
    }
    // Walk up to the deepest existing ancestor, collecting the non-existent tail.
    let mut existing = path;
    let mut tail: Vec<Component<'_>> = Vec::new();
    loop {
        if existing.exists() {
            break;
        }
        match (existing.parent(), existing.components().next_back()) {
            (Some(parent), Some(comp)) => {
                tail.push(comp);
                existing = parent;
            }
            _ => break,
        }
    }
    let mut resolved = canonicalize_best_effort(existing);
    for component in tail.into_iter().rev() {
        resolved.push(component);
    }
    resolved
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_policy_id_round_trips() {
        let encoded = workspace_policy_id("alice");
        assert_eq!(encoded, "openhuman.profile:alice");
        assert_eq!(profile_id_from_policy_id(&encoded), Some("alice"));
    }

    #[test]
    fn profile_id_from_policy_id_rejects_non_profile_ids() {
        // Worktree-isolation / test ids and empty strings are not profiles.
        assert_eq!(profile_id_from_policy_id("test-worktree"), None);
        assert_eq!(profile_id_from_policy_id(""), None);
        assert_eq!(profile_id_from_policy_id("openhuman.profile:"), None);
        assert_eq!(
            profile_id_from_policy_id("openhuman.profile:bob"),
            Some("bob")
        );
    }

    // ── Cross-profile classifier (1b) ─────────────────────────────────────

    fn profiles_layout() -> (tempfile::TempDir, PathBuf) {
        let action = tempfile::tempdir().expect("action tempdir");
        let profiles = action.path().join("profiles");
        for id in ["alice", "bob"] {
            std::fs::create_dir_all(profiles.join(id)).unwrap();
        }
        let action_dir = action.path().to_path_buf();
        (action, action_dir)
    }

    #[test]
    fn same_profile_target_is_allowed() {
        let (_g, action) = profiles_layout();
        let target = action.join("profiles").join("alice").join("notes.txt");
        assert_eq!(
            classify_cross_profile_target(&action, "alice", &target),
            CrossProfileDecision::Allow
        );
    }

    #[test]
    fn other_profile_target_is_blocked() {
        let (_g, action) = profiles_layout();
        let target = action.join("profiles").join("bob").join("secret.txt");
        assert_eq!(
            classify_cross_profile_target(&action, "alice", &target),
            CrossProfileDecision::Block {
                other_id: "bob".into()
            }
        );
    }

    #[test]
    fn target_outside_profiles_root_is_allowed() {
        let (_g, action) = profiles_layout();
        // A plain file under action_dir (the shared workspace) — not under
        // profiles/ at all.
        let target = action.join("scratch.txt");
        assert_eq!(
            classify_cross_profile_target(&action, "alice", &target),
            CrossProfileDecision::Allow
        );
    }

    #[test]
    fn nonexistent_sibling_target_is_blocked_via_ancestor() {
        let (_g, action) = profiles_layout();
        // File does not exist yet, but its parent (profiles/bob) does → the
        // deepest-existing-ancestor resolution still classifies it as bob's.
        let target = action
            .join("profiles")
            .join("bob")
            .join("nested")
            .join("fresh.txt");
        assert_eq!(
            classify_cross_profile_target(&action, "alice", &target),
            CrossProfileDecision::Block {
                other_id: "bob".into()
            }
        );
    }

    #[test]
    fn relative_traversal_into_sibling_is_blocked() {
        let (_g, action) = profiles_layout();
        // A relative `../bob/x` composed from the active profile's own dir.
        let target = action
            .join("profiles")
            .join("alice")
            .join("..")
            .join("bob")
            .join("x.txt");
        assert_eq!(
            classify_cross_profile_target(&action, "alice", &target),
            CrossProfileDecision::Block {
                other_id: "bob".into()
            }
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlink_into_sibling_profile_is_blocked() {
        use std::os::unix::fs::symlink;
        let (_g, action) = profiles_layout();
        // Inside alice, a symlink `link -> ../bob`. Writing `link/hijack.txt`
        // must resolve to bob's dir and block.
        let alice = action.join("profiles").join("alice");
        let bob = action.join("profiles").join("bob");
        symlink(&bob, alice.join("link")).unwrap();
        let target = alice.join("link").join("hijack.txt");
        assert_eq!(
            classify_cross_profile_target(&action, "alice", &target),
            CrossProfileDecision::Block {
                other_id: "bob".into()
            }
        );
    }

    #[test]
    fn profiles_root_itself_is_allowed() {
        // `profiles/` (no owner component) is not a sibling write.
        let (_g, action) = profiles_layout();
        let target = action.join("profiles");
        assert_eq!(
            classify_cross_profile_target(&action, "alice", &target),
            CrossProfileDecision::Allow
        );
    }

    // ── Shell command scan (1b) ───────────────────────────────────────────

    #[test]
    fn scan_command_allows_same_profile_and_bare_tokens() {
        let (_g, action) = profiles_layout();
        let cwd = action.join("profiles").join("alice");
        // Relative writes under cwd, plain words, and reads under cwd are fine.
        assert_eq!(
            scan_command_for_cross_profile("echo hi > notes.txt", &cwd, &action, "alice"),
            None
        );
        assert_eq!(
            scan_command_for_cross_profile("ls -la sub/dir", &cwd, &action, "alice"),
            None
        );
    }

    #[test]
    fn scan_command_blocks_absolute_sibling_target() {
        let (_g, action) = profiles_layout();
        let cwd = action.join("profiles").join("alice");
        let sibling = action.join("profiles").join("bob").join("loot.txt");
        let command = format!("cat secret > {}", sibling.display());
        assert_eq!(
            scan_command_for_cross_profile(&command, &cwd, &action, "alice"),
            Some("bob".to_string())
        );
    }

    #[test]
    fn scan_command_blocks_relative_traversal_into_sibling() {
        let (_g, action) = profiles_layout();
        let cwd = action.join("profiles").join("alice");
        assert_eq!(
            scan_command_for_cross_profile("cp x ../bob/y", &cwd, &action, "alice"),
            Some("bob".to_string())
        );
    }
}
