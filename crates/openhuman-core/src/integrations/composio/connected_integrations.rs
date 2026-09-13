//! Connected-integrations discovery: the process-wide [`cache`] fronting a
//! mode-aware backend/direct [`fetch`] (whose uncached backend walk lives in
//! [`fetch_uncached`] since it is one large, sequential routine).

mod cache;
mod fetch;
mod fetch_uncached;

#[cfg(test)]
#[path = "connected_integrations_connectable_slug_tests_tests.rs"]
mod connectable_slug_tests;

#[cfg(test)]
#[path = "connected_integrations_catalog_description_tests_tests.rs"]
mod catalog_description_tests;

#[cfg(test)]
pub(crate) use cache::composio_cache_test_lock;
pub(crate) use cache::{
    cached_active_integrations, cached_active_integrations_including_expired, connected_set_hash,
    invalidate_connected_integrations_cache, sync_cache_with_connections,
};
pub(crate) use fetch::{
    fetch_connected_integrations, fetch_connected_integrations_status, fetch_toolkit_actions,
    FetchConnectedIntegrationsStatus,
};

// Brought into this module's own namespace (private `use`, not `pub use`)
// so `connected_integrations_connectable_slug_tests_tests.rs` /
// `connected_integrations_catalog_description_tests_tests.rs` — declared as
// direct child modules of `connected_integrations` above — can still reach
// these via a plain `use super::<name>;`, exactly as when this was one
// un-split file. See each item's `pub(super)` in its owning submodule.
#[cfg(test)]
pub(crate) use cache::{cache_key, CachedIntegrations, CACHE_TTL, INTEGRATIONS_CACHE};
#[cfg(test)]
pub(crate) use fetch::{connectable_toolkit_slugs, resolve_toolkit_description};
