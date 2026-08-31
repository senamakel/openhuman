//! Tools *about* the tool surface itself.
//!
//! Two members: [`tool_search`], the lookup half of
//! [`ToolExposure::Deferred`](crate::openhuman::tools::ToolExposure). It sits
//! in its own family rather than under `system/` because it is not a capability
//! the host offers the user — it is the model asking what it is able to do.

pub mod collapse;
pub mod tool_search;

pub use collapse::{
    any_external_effect, args_without_action, merge_action_schemas, resolve,
    strictest_permission, unknown_action_message, CollapsedAction,
};
pub use tool_search::{
    bind_tool_search_index, strip_deferred_from_visible, ToolSearchHandle, ToolSearchIndex,
    ToolSearchTool,
    TOOL_SEARCH_NAME,
};
