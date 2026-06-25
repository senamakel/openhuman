//! CCR (Compress-Cache-Retrieve) — original storage + retrieval markers.

pub mod marker;
pub mod store;

pub use marker::{
    format_marker, parse_markers, recovery_footer, LEGACY_RETRIEVE_TOOL_NAME, RETRIEVE_TOOL_NAME,
};
pub use store::{
    configure, enable_disk_tier, offload, retrieve, retrieve_range, short_hash, stats, RangeUnit,
};
