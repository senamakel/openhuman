//! Tools *about* the tool surface itself.
//!
//! One member so far: [`tool_search`], the lookup half of
//! [`ToolExposure::Deferred`](crate::openhuman::tools::ToolExposure). It sits
//! in its own family rather than under `system/` because it is not a capability
//! the host offers the user — it is the model asking what it is able to do.

pub mod tool_search;

pub use tool_search::{
    strip_deferred_from_visible, ToolSearchHandle, ToolSearchIndex, ToolSearchTool,
    TOOL_SEARCH_NAME,
};
