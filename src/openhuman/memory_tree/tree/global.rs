//! Global tree policy over the generic tree engine.
//!
//! This is now a thin module root that gathers the remaining global-specific
//! algorithms while avoiding a dedicated `tree/global/` directory.

#[path = "global_digest.rs"]
pub mod digest;
#[path = "global_recap.rs"]
pub mod recap;
#[path = "global_seal.rs"]
pub mod seal;

pub use crate::openhuman::memory_store::trees::registry;
pub use crate::openhuman::memory_store::trees::get_or_create_global_tree;
pub use crate::openhuman::memory_tree::tree::factory::GLOBAL_SCOPE;
pub use digest::{end_of_day_digest, DigestOutcome};
pub use recap::{recap, RecapOutput};

/// Number of L0 (daily) nodes that seal into one L1 (weekly) node.
pub const WEEKLY_SEAL_THRESHOLD: usize = 7;

/// Number of L1 (weekly) nodes that seal into one L2 (monthly) node.
pub const MONTHLY_SEAL_THRESHOLD: usize = 4;

/// Number of L2 (monthly) nodes that seal into one L3 (yearly) node.
pub const YEARLY_SEAL_THRESHOLD: usize = 12;

/// Token budget passed into the summariser for global-tree seals.
pub const GLOBAL_TOKEN_BUDGET: u32 = 4_000;
