//! The Composio vocabulary the host's integration code shares: toolkit
//! catalogs (curated action lists and descriptions), connected-identity
//! profiles, per-toolkit user scope preferences and the
//! normalised task shape.
//!
//! These shapes live beside the integration that produces and reads them.
//! They are plain data and pure functions: no HTTP, no persistence.

pub mod catalogs;
pub mod profile;
pub mod scopes;
pub mod tasks;

pub use profile::{
    canonicalize, normalize_connection_identifier, render_connected_identities_section,
    ConnectedIdentity, IdentityKind, ProviderUserProfile,
};
pub use scopes::{
    action_slug_matches, agent_ready_toolkits, canonical_action_slug, classify_unknown,
    find_curated, toolkit_from_slug, CuratedTool, ToolScope, UserScopePref,
};
pub use tasks::{GithubFetchMode, NormalizedTask, TaskContainer, TaskFetchFilter, TaskKind};

pub use catalogs::{
    catalog_for_toolkit, curated_scope_for, has_native_provider, is_action_visible_with_pref,
    toolkit_description,
};
