//! Well-known memory namespace names, as inert strings.
//!
//! # Why these are here and not with the code that fills them
//!
//! Each of these constants names a bucket that a **gated** producer writes and
//! an **always-compiled** caller reads back. `CONVERSATION_RAW_NAMESPACE` is
//! written by `agent::learning::transcript_ingest` but named by the turn
//! autosave in `session::turn::core`; `REFLECTIONS_NAMESPACE` is written by
//! `agent::learning::reflection` but named by the prompt assembly in
//! `session::turn::context`. Both readers go through `Arc<dyn Memory>`, which
//! is contract vocabulary and compiles in every build — so with the `memory`
//! family off the *only* thing they were missing was the string.
//!
//! A `&'static str` cannot fail without a driver, so this follows
//! `skills::types`' rule: inert parts of a gated domain live in a
//! dependency-free submodule and stay ungated. The owning modules re-export
//! them, so every historical path keeps resolving and there is still one
//! definition of each.

/// Raw, unprocessed conversation turns.
///
/// These used to be stored with an empty namespace, which `sanitize_namespace`
/// maps to `global` — the same namespace the default recall reads. Every
/// message a user ever sent therefore accumulated in the one bucket retrieval
/// scanned in full, so recall got steadily slower for exactly the users who
/// used the product most; benchmarked at ~10k messages the scan dominated
/// per-turn cost.
///
/// Distinct from the conversation-memory namespace, which holds *derived*
/// durable facts rather than raw turns. These are the unprocessed messages:
/// still queryable on demand by naming this namespace, but no longer swept
/// into every default-namespace read.
pub const CONVERSATION_RAW_NAMESPACE: &str = "conversation_raw";

/// Memory namespace + custom-category tag for explicit user reflections.
///
/// Distinct from `learning_observations` (agent-extracted) and `user_profile`
/// (preference facts) — these are sentences the user authored about themselves
/// that should steer future agent behaviour.
pub const REFLECTIONS_NAMESPACE: &str = "learning_reflections";
