//! Active-profile identity plumbing (and, from 1b, the cross-profile write guard).
//!
//! When a dedicated-workspace profile is active, its id is carried to the tool
//! layer inside the
//! [`WorkspaceDescriptor`](tinyagents::harness::workspace::WorkspaceDescriptor)'s
//! `policy_id` field as `openhuman.profile:<id>`. [`workspace_policy_id`] and
//! [`profile_id_from_policy_id`] are the single encode/decode pair so the
//! session builder and the tool gates can never drift on the wire format.

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
}
