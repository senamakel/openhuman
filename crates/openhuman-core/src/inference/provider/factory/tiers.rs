//! Managed workload vocabulary: `hint:*` role aliases, retired tier slugs,
//! passthrough detection, and vision capability for the managed backend.
//!
//! The managed backend serves OpenRouter model ids only. Workload **roles**
//! (`chat`, `reasoning`, `coding`, …) remain the routing vocabulary — a role
//! picks its `*_provider` route — but on the managed backend every role runs on
//! one concrete model: the pinned default (`config.default_model`) or
//! [`MODEL_MANAGED_DEFAULT`]. The old `hint:chat`-style tier slugs are accepted
//! as aliases of their role so un-migrated callers keep routing.

use super::*;
use crate::config::{legacy_tier_role, MANAGED_MULTIMODAL_MODELS, MODEL_MANAGED_DEFAULT};

/// Whether `model` is a managed alias rather than a concrete model id: a
/// `hint:*` role marker or a retired tier slug.
pub(super) fn is_abstract_tier_model(model: &str) -> bool {
    let trimmed = model.trim();
    trimmed.starts_with("hint:") || legacy_tier_role(trimmed).is_some()
}

/// The workload role a `hint:*` marker names, if it is one of ours.
fn hint_role(hint: &str) -> Option<&'static str> {
    match hint.strip_prefix("hint:")? {
        "chat" => Some("chat"),
        "reasoning" => Some("reasoning"),
        "agentic" => Some("agentic"),
        "burst" => Some("burst"),
        "coding" => Some("coding"),
        "vision" => Some("vision"),
        "summarization" => Some("summarization"),
        "subconscious" => Some("subconscious"),
        _ => None,
    }
}

/// The concrete model every managed role runs on: the user's pinned default,
/// else [`MODEL_MANAGED_DEFAULT`].
pub(crate) fn managed_default_model(config: &Config) -> String {
    pinned_managed_default_model(config).unwrap_or_else(|| MODEL_MANAGED_DEFAULT.to_string())
}

/// Resolve a model hint (e.g. `"hint:reasoning"`), a retired tier slug, or a
/// concrete model id to the model string the provider router would use —
/// without constructing the provider. A BYOK route yields its own model; a
/// managed route yields [`managed_default_model`]; a concrete id passes through.
pub fn resolve_model_for_hint(hint_or_tier: &str, config: &Config) -> String {
    let trimmed = hint_or_tier.trim();
    let role = match hint_role(trimmed).or_else(|| legacy_tier_role(trimmed)) {
        Some(role) => role,
        None => {
            // A concrete id (catalog model, BYOK id) resolves to itself.
            return trimmed.to_string();
        }
    };

    let provider_string = provider_for_role(role, config);
    let ps = provider_string.trim();
    if ps.is_empty() || ps == "cloud" || ps == PROVIDER_OPENHUMAN || ps == BYOK_INCOMPLETE_SENTINEL
    {
        let model = managed_default_model(config);
        log::debug!(
            "[providers][resolve-hint] hint={} role={} managed resolves to model={}",
            hint_or_tier,
            role,
            model
        );
        model
    } else if let Some(idx) = ps.find(':') {
        let model_with_temp = &ps[idx + 1..];
        let (model, _) = split_model_and_temperature(model_with_temp);
        model
    } else {
        ps.to_string()
    }
}

/// Map a `hint:*` marker or retired tier slug to the workload **role** whose
/// configured provider serves it. Concrete model ids and unknown strings fall
/// back to `"chat"`.
///
/// Kept deliberately small and standalone (no `Config`): callers that select
/// a model *per unit of work* (a tinyflows `agent` node pinning
/// `config.model = "hint:reasoning"`) turn that back into the role, then call
/// `create_chat_model` with it so the completion follows the role's route.
pub fn role_for_model_tier(hint_or_tier: &str) -> &'static str {
    let trimmed = hint_or_tier.trim();
    match hint_role(trimmed).or_else(|| legacy_tier_role(trimmed)) {
        // The background subconscious *role* keeps its own `subconscious_provider`
        // (see `resolve_model_for_hint`), but as a model-tier spelling it has
        // always ridden the chat route.
        Some("subconscious") => "chat",
        Some(role) => role,
        None => "chat",
    }
}

/// The user's pinned managed **default model** — `config.default_model` when
/// it names a concrete catalog model (e.g. `openrouter/deepseek/deepseek-v4-flash`)
/// rather than a `hint:*` marker or a retired tier slug.
///
/// The Routing page writes a catalog id here as the "default model": the one
/// every managed workload runs on. Hints and retired tiers are not pins — they
/// are the old "let the backend pick" contract — so they yield `None`.
pub(crate) fn pinned_managed_default_model(config: &Config) -> Option<String> {
    let model = config.default_model.as_deref()?.trim();
    if model.is_empty() || is_known_openhuman_tier(model) {
        return None;
    }
    Some(model.to_string())
}

/// Whether `model` is a managed alias the backend must never receive verbatim:
/// a `hint:*` role marker we translate, or a retired tier slug.
pub(crate) fn is_known_openhuman_tier(model: &str) -> bool {
    let trimmed = model.trim();
    hint_role(trimmed).is_some() || legacy_tier_role(trimmed).is_some()
}

/// Return whether `model` is a raw model id that must be forwarded **verbatim**
/// to provider construction rather than mapped onto a managed role.
///
/// A raw passthrough id is any **non-empty** string that is neither a `hint:*`
/// alias nor a retired tier slug — the model ids a user pins directly on an
/// agent/node (e.g. `"openrouter/deepseek/deepseek-v4-pro"`). The managed
/// backend is authoritative over their validity, so the core must **not**
/// silently collapse them onto the default (issue #4598).
pub(crate) fn is_raw_passthrough_model(model: &str) -> bool {
    let trimmed = model.trim();
    !trimmed.is_empty() && !trimmed.starts_with("hint:") && !is_known_openhuman_tier(trimmed)
}

/// Vision (image-input) capability for a managed model or role alias.
///
/// The managed backend does not advertise per-model capabilities, so the core
/// owns this. [`MODEL_MANAGED_DEFAULT`] (DeepSeek V4 Flash on the managed
/// backend) accepts images, as do the `vision` and `reasoning` role aliases
/// (and their retired tier slugs) that always ran a multimodal model, and
/// every exact id in [`MANAGED_MULTIMODAL_MODELS`] — the OpenRouter
/// passthrough models the media agents (`vision_agent`, `image_agent`,
/// `video_agent`) are pinned to now that `hint:vision` / `vision-v1` is
/// deprecated (regression R4: the retired hint silently fell back to the
/// chat default on managed routes, so an agent still pinned to it lost image
/// forwarding with no error). Any other pinned catalog id is covered by the
/// user's `model_registry.vision` flag
/// ([`crate::inference::model_context::model_vision_enabled`]).
pub(crate) fn oh_tier_supports_vision(model: &str) -> bool {
    let trimmed = model.trim();
    if trimmed == MODEL_MANAGED_DEFAULT || MANAGED_MULTIMODAL_MODELS.contains(&trimmed) {
        return true;
    }
    matches!(
        hint_role(trimmed).or_else(|| legacy_tier_role(trimmed)),
        Some("vision") | Some("reasoning")
    )
}
