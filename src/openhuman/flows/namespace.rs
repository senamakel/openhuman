//! A flow's private memory namespace — the naming rule, on its own.
//!
//! # Why this is not in `memory_tools`
//!
//! It was, and it is the same code. What moved is only *where*, and for one
//! reason: the naming rule is a pure `format!` over a `flow_id`, while
//! everything else in `memory_tools` needs a driver. When the `memory` family
//! is compiled out the tools go with it, but `flows` still needs the prefix —
//! `flows_delete`'s cleanup and the run-digest subscriber both name it — so
//! keeping the rule in the gated module would have forced either a stub twin
//! of a `format!` call or a `#[cfg]` on every caller.
//!
//! This is `skills::types`' rule again: a domain's inert parts live in a
//! dependency-free submodule and stay ungated; only the behaviour is gated.
//!
//! # The security invariant is unchanged
//!
//! [`flow_namespace`] is still the **only** place in the codebase that may
//! construct this namespace string, and moving it kept that true — there is
//! one definition, not two. Every caller (the `flow_memory_recall` /
//! `flow_memory_remember` agent tools, the post-run digest subscriber in
//! `flows::bus`, and the `flows_delete` cleanup hook) goes through this
//! function with a `flow_id`, never with a caller-supplied raw namespace. A
//! flow can therefore never write to, or be told the name of, any memory
//! namespace but its own.

/// Prefix on every flow's private memory namespace.
///
/// Underscore rather than a colon so the string survives round-tripping
/// through every `Memory` method that treats a colon as a separator: one
/// prefix, agreed on by recall, list and clear, with no silent mismatch.
pub const FLOW_MEMORY_NAMESPACE_PREFIX: &str = "flow_";

/// Builds a flow's private, profile-independent memory namespace from a
/// `flow_id`.
///
/// See the module docs for the security invariant this function carries.
#[must_use]
pub fn flow_namespace(flow_id: &str) -> String {
    format!("{FLOW_MEMORY_NAMESPACE_PREFIX}{flow_id}")
}
