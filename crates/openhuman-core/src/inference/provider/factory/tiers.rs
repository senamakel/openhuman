//! Managed tier / `hint:*` vocabulary: tier↔role lookups, passthrough
//! detection, and per-tier vision capability for the managed backend.

use super::*;

pub(super) fn is_abstract_tier_model(model: &str) -> bool {
    use crate::config::{
        MODEL_AGENTIC_V1, MODEL_BURST_V1, MODEL_CHAT_V1, MODEL_CODING_V1, MODEL_REASONING_QUICK_V1,
        MODEL_REASONING_V1, MODEL_SUMMARIZATION_V1, MODEL_VISION_V1,
    };
    let trimmed = model.trim();
    trimmed == MODEL_REASONING_V1
        || trimmed == MODEL_REASONING_QUICK_V1
        || trimmed == MODEL_CHAT_V1
        || trimmed == MODEL_AGENTIC_V1
        || trimmed == MODEL_BURST_V1
        || trimmed == MODEL_CODING_V1
        || trimmed == MODEL_VISION_V1
        || trimmed == MODEL_SUMMARIZATION_V1
}

/// Resolve a model hint (e.g. `"hint:reasoning"`) or tier name to the
/// concrete model string that the provider router would use — without
/// constructing the actual provider.  Returns the provider-string prefix
/// (e.g. `"openai"`) concatenated with the model when a BYOK provider is
/// active, or the bare tier name for the managed OpenHuman backend.
pub fn resolve_model_for_hint(hint_or_tier: &str, config: &Config) -> String {
    let hint_to_tier: &[(&str, &str)] = &[
        ("reasoning", crate::config::MODEL_REASONING_V1),
        ("chat", crate::config::MODEL_CHAT_V1),
        ("agentic", crate::config::MODEL_AGENTIC_V1),
        ("burst", crate::config::MODEL_BURST_V1),
        ("coding", crate::config::MODEL_CODING_V1),
        ("vision", crate::config::MODEL_VISION_V1),
        ("summarization", crate::config::MODEL_SUMMARIZATION_V1),
        // Background subconscious workload rides the lightweight chat tier on the
        // managed backend; its `subconscious` *role* (handled below) still selects
        // the provider via `subconscious_provider`.
        ("subconscious", crate::config::MODEL_CHAT_V1),
    ];
    let tier_to_role: &[(&str, &str)] = &[
        (crate::config::MODEL_REASONING_V1, "reasoning"),
        (crate::config::MODEL_CHAT_V1, "chat"),
        (crate::config::MODEL_REASONING_QUICK_V1, "chat"),
        (crate::config::MODEL_AGENTIC_V1, "agentic"),
        (crate::config::MODEL_BURST_V1, "burst"),
        (crate::config::MODEL_CODING_V1, "coding"),
        (crate::config::MODEL_VISION_V1, "vision"),
        (crate::config::MODEL_SUMMARIZATION_V1, "summarization"),
    ];

    let (tier, role) = if let Some(hint_key) = hint_or_tier.strip_prefix("hint:") {
        let tier = hint_to_tier
            .iter()
            .find(|(k, _)| *k == hint_key)
            .map(|(_, v)| *v)
            .unwrap_or(hint_or_tier);
        // Background workloads map to a tier *model* but must keep their own
        // role so `provider_for_role` reads their dedicated `*_provider` field
        // rather than the chat-tier provider their model happens to share.
        let role = match hint_key {
            "subconscious" => "subconscious",
            _ => tier_to_role
                .iter()
                .find(|(k, _)| *k == tier)
                .map(|(_, v)| *v)
                .unwrap_or(hint_key),
        };
        (tier, role)
    } else {
        let role = tier_to_role
            .iter()
            .find(|(k, _)| *k == hint_or_tier)
            .map(|(_, v)| *v)
            .unwrap_or("chat");
        (hint_or_tier, role)
    };

    let provider_string = provider_for_role(role, config);
    let ps = provider_string.trim();
    if ps.is_empty() || ps == "cloud" || ps == PROVIDER_OPENHUMAN || ps == BYOK_INCOMPLETE_SENTINEL
    {
        tier.to_string()
    } else if let Some(idx) = ps.find(':') {
        let model_with_temp = &ps[idx + 1..];
        let (model, _) = split_model_and_temperature(model_with_temp);
        model
    } else {
        ps.to_string()
    }
}

/// Map a managed tier name (or `hint:*` string) to the workload **role** whose
/// configured provider serves it.
///
/// This is the inverse of the role→tier routing `create_chat_model` does:
/// callers that select a model *per unit of work by tier* (e.g. a tinyflows
/// `agent` node pinning `config.model = "reasoning-v1"`) use this to turn that
/// tier back into the role, then call [`create_chat_model`] with it — so the
/// completion routes to that tier on the managed backend (or the role's BYOK
/// model) instead of some caller default. Unknown strings fall back to `"chat"`.
///
/// Kept deliberately small and standalone (no `Config`) — it is a pure lookup
/// over the tier constants, mirroring the `tier_to_role` table inside
/// [`resolve_model_for_hint`].
pub fn role_for_model_tier(hint_or_tier: &str) -> &'static str {
    use crate::config::{
        MODEL_AGENTIC_V1, MODEL_BURST_V1, MODEL_CHAT_V1, MODEL_CODING_V1, MODEL_REASONING_QUICK_V1,
        MODEL_REASONING_V1, MODEL_SUMMARIZATION_V1, MODEL_VISION_V1,
    };

    // Normalise a `hint:*` alias to its concrete tier first.
    let tier = match hint_or_tier.strip_prefix("hint:") {
        Some("reasoning") => MODEL_REASONING_V1,
        Some("chat") => MODEL_CHAT_V1,
        Some("agentic") => MODEL_AGENTIC_V1,
        Some("burst") => MODEL_BURST_V1,
        Some("coding") => MODEL_CODING_V1,
        Some("vision") => MODEL_VISION_V1,
        Some("summarization") => MODEL_SUMMARIZATION_V1,
        // Background subconscious rides the chat tier for its model.
        Some("subconscious") => MODEL_CHAT_V1,
        Some(_) => hint_or_tier,
        None => hint_or_tier,
    };

    match tier {
        MODEL_REASONING_V1 => "reasoning",
        MODEL_CHAT_V1 | MODEL_REASONING_QUICK_V1 => "chat",
        MODEL_AGENTIC_V1 => "agentic",
        MODEL_BURST_V1 => "burst",
        MODEL_CODING_V1 => "coding",
        MODEL_VISION_V1 => "vision",
        MODEL_SUMMARIZATION_V1 => "summarization",
        _ => "chat",
    }
}

/// Return whether `model` is a recognized OpenHuman backend tier name.
///
/// Used to guard against stale `default_model` values (e.g. set by older UI
/// versions) that the backend would reject with HTTP 400.  The known tiers are
/// the constants in `crate::config`; the four `hint:*` strings that
/// `make_openhuman_backend` actually translates are also accepted.  An
/// unrecognized `hint:*` value is intentionally rejected so the factory falls
/// back to the platform default instead of forwarding an untranslated string
/// to the backend.
pub(crate) fn is_known_openhuman_tier(model: &str) -> bool {
    use crate::config::{
        MODEL_AGENTIC_V1, MODEL_BURST_V1, MODEL_CHAT_V1, MODEL_CODING_V1, MODEL_REASONING_QUICK_V1,
        MODEL_REASONING_V1, MODEL_SUMMARIZATION_V1, MODEL_VISION_V1,
    };
    matches!(
        model,
        MODEL_REASONING_V1
            | MODEL_CHAT_V1
            | MODEL_AGENTIC_V1
            | MODEL_BURST_V1
            | MODEL_CODING_V1
            | MODEL_REASONING_QUICK_V1
            | MODEL_SUMMARIZATION_V1
            | MODEL_VISION_V1
            | "hint:reasoning"
            | "hint:chat"
            | "hint:agentic"
            | "hint:burst"
            | "hint:coding"
            | "hint:summarization"
            | "hint:vision"
    )
}

/// Return whether `model` is a raw BYOK/custom model id that must be forwarded
/// **verbatim** to provider construction rather than mapped onto a managed tier.
///
/// A raw passthrough id is any **non-empty** string that is neither a `hint:*`
/// alias nor a known managed tier ([`is_known_openhuman_tier`]) — i.e. the model
/// ids a user pins directly on an agent/node (e.g. `"claude-opus-4"`). The
/// OpenHuman backend preserves such ids verbatim
/// (the managed model's blank-id normalization) and is authoritative over
/// their validity, so the core must **not** silently collapse them onto
/// `reasoning-v1` (issue #4598). Managed tiers and every `hint:*` string return
/// `false` so their existing resolution is untouched.
pub(crate) fn is_raw_passthrough_model(model: &str) -> bool {
    let trimmed = model.trim();
    !trimmed.is_empty() && !trimmed.starts_with("hint:") && !is_known_openhuman_tier(trimmed)
}

/// Per-tier vision (image-input) capability for the managed OpenHuman backend.
///
/// The remote managed backend (`api.tinyhumans.ai`) does not advertise per-tier
/// capabilities, so the core maintains this map itself. Accepts both the tier
/// constants and their `hint:*` forms (callers may pass either pre- or
/// post-resolution).
///
/// `reasoning-v1` and the dedicated `vision-v1` tier are multimodal; every
/// other tier returns `false` — flip an individual arm to `true` once that tier
/// is confirmed multimodal on the backend. This is the **only** place to change
/// managed-model vision; BYOK/custom models are handled separately by the
/// user-set `model_registry.vision` flag
/// ([`crate::inference::model_context::model_vision_enabled`]).
pub(crate) fn oh_tier_supports_vision(model: &str) -> bool {
    use crate::config::{
        MODEL_AGENTIC_V1, MODEL_BURST_V1, MODEL_CHAT_V1, MODEL_CODING_V1, MODEL_REASONING_QUICK_V1,
        MODEL_REASONING_V1, MODEL_SUMMARIZATION_V1, MODEL_VISION_V1,
    };
    match model {
        MODEL_REASONING_V1 | "hint:reasoning" => true,
        // Dedicated multimodal tier — the managed backend serves this with the
        // vision flag enabled. This is what the vision sub-agent rides on.
        MODEL_VISION_V1 | "hint:vision" => true,
        MODEL_CHAT_V1 | "hint:chat" => false,
        MODEL_REASONING_QUICK_V1 => false,
        MODEL_AGENTIC_V1 | "hint:agentic" => false,
        // Burst is a text-only tier.
        MODEL_BURST_V1 | "hint:burst" => false,
        MODEL_CODING_V1 | "hint:coding" => false,
        MODEL_SUMMARIZATION_V1 | "hint:summarization" => false,
        _ => false,
    }
}
