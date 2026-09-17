//! Shell-command classification internals used by [`super::SecurityPolicy`]'s
//! command-risk and allowlist gates.
//!
//! Split by responsibility:
//! - [`env_guard`]: leading inline env-assignment detection.
//! - [`command_name`]: basename normalization and "executes arbitrary code" bases.
//! - [`quoting`]: quote-aware segment splitting and unquoted-character detection.
//! - [`classification`]: read/write/network/install/destructive bucket lists
//!   and the hidden-execution structural guard.

mod classification;
mod command_name;
mod env_guard;
mod quoting;

pub(super) use classification::{classify_segment, has_hidden_execution};
pub(super) use command_name::{command_basename, is_command_executor, normalized_command_name};
pub(super) use env_guard::{
    has_dangerous_env_prefix, has_leading_env_assignment, skip_env_assignments,
};
pub(super) use quoting::{
    contains_unquoted_char, contains_unquoted_single_ampersand, split_unquoted_segments,
};
