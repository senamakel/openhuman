//! Composio-backed sync pipelines.
//!
//! This module owns the "pull upstream provider data into memory" side of
//! Composio integrations:
//!
//! - provider sync implementations (`providers/*/provider.rs`, `sync.rs`)
//! - periodic scheduler (`periodic.rs`)
//! - trigger / connection-created event subscribers (`bus.rs`)
//! - sync-state persistence and profile-to-memory shaping
//!
//! The sibling [`crate::openhuman::composio`] domain still owns auth,
//! connection management, action execution, and general Composio RPC/tool
//! surfaces. This submodule is specifically the memory-sync half of that
//! integration boundary.

pub mod bus;
pub mod periodic;
pub mod providers;

pub use bus::{
    register_composio_trigger_subscriber, ComposioConfigChangedSubscriber,
    ComposioTriggerSubscriber,
};
pub use periodic::{record_sync_success, start_periodic_sync};
pub use providers::{
    all_providers as all_composio_sync_providers, get_provider as get_composio_sync_provider,
    init_default_providers as init_default_composio_sync_providers, ComposioProvider,
    ProviderContext, ProviderUserProfile, SyncOutcome, SyncReason,
};
