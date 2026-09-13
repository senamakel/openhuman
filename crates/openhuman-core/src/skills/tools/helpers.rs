//! Shared argument-parsing and skill-allowlist helpers for the workflow tools
//! in [`super::read`] and [`super::write`].

use std::path::Path;

pub(in crate::skills) fn read_required_str(
    args: &serde_json::Value,
    key: &str,
) -> anyhow::Result<String> {
    args.get(key)
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .ok_or_else(|| anyhow::anyhow!("missing required string argument `{key}`"))
}

/// Read the target workflow id, accepting the legacy `skill_id` key as an
/// alias for `workflow_id` so callers from before the rename still work.
pub(in crate::skills) fn read_workflow_id(args: &serde_json::Value) -> anyhow::Result<String> {
    read_required_str(args, "workflow_id")
        .or_else(|_| read_required_str(args, "skill_id"))
        .map_err(|_| anyhow::anyhow!("missing required string argument `workflow_id`"))
}

/// Skill/workflow allowlist applied per agent profile. `None` = all skills are
/// visible (the default). `Some(set)` restricts to the named `dir_name` slugs.
pub(in crate::skills) type SkillAllowlist = Option<std::collections::HashSet<String>>;

/// Whether `dir_name` passes the optional per-profile skill allowlist.
pub(in crate::skills) fn skill_allowed(allowlist: &SkillAllowlist, dir_name: &str) -> bool {
    match allowlist {
        None => true,
        Some(set) => set.contains(dir_name),
    }
}

/// Whether `skill_id` names a skill compiled into this binary.
///
/// Builtin bundles are exempt from the per-profile allowlist for the same
/// reason profile-local ones are: the allowlist scopes **user content**, and
/// these are neither the user's nor scoped — they come from a `const` table in
/// this build and one of them (`flow-authoring`) is the reference manual an
/// agent's own system prompt points it at. A profile that narrowed its skills
/// would otherwise leave that agent pointing at a page it is refused.
///
/// This widens nothing a user chose: no RPC and no config can add a row to that
/// table (see `skills::bundled`), so the exempt set is fixed at compile time.
pub(in crate::skills) fn is_builtin_skill(skill_id: &str) -> bool {
    super::super::bundled::BUNDLED
        .iter()
        .any(|s| s.dir_name == skill_id)
}

/// Whether `skill_id` is usable given the profile's allowlist AND its private
/// skills. A profile's own (profile-local) skills are implicitly allowed for
/// their owner — they bypass the `allowed_skills` allowlist, mirroring
/// `list_workflows`. `profile_local_ids` is empty for the profile-less session
/// and other profiles, so this reduces to [`skill_allowed`] there.
pub(in crate::skills) fn skill_allowed_including_profile(
    allowlist: &SkillAllowlist,
    profile_local_ids: &std::collections::HashSet<String>,
    _workspace_dir: &Path,
    _profile_skills_root: Option<&Path>,
    skill_id: &str,
) -> bool {
    is_builtin_skill(skill_id)
        || profile_local_ids.contains(skill_id)
        || skill_allowed(allowlist, skill_id)
}
