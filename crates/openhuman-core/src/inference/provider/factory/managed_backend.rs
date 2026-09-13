//! The managed OpenHuman backend as a crate-native `ChatModel`: tier pinning per
//! workload role, `hint:*` translation, and the single managed egress emission.

use super::*;

/// Canonical managed-backend tier for a specialised workload role.
///
/// The managed backend otherwise derives its model from `config.default_model`
/// (which defaults to the `chat-v1` tier), so a tier-specific workload whose
/// per-workload provider is unset would silently inherit the global default —
/// e.g. the `code_executor` sub-agent (`hint = "coding"`) would run on `chat-v1`
/// instead of the dedicated `coding-v1` tier, defeating the whole point of the
/// hint. The `hint:<tier>` translation in [`make_openhuman_backend`] only fires
/// when the *model string itself* is `hint:coding`; here the model originates
/// from `default_model`, so the workload role is the only signal left and must
/// be mapped explicitly.
///
/// Returns `Some(tier)` for the specialised roles that map 1:1 to a managed
/// tier (`reasoning`, `agentic`, `coding`, `vision`, `subconscious`). Returns
/// `None` for:
///
/// - the generic `chat` role (and any other background/unknown role), which
///   keeps inheriting `default_model`: the front-line chat turn and legacy
///   `default_model = "reasoning-v1"` installs deliberately fall through to the
///   `chat` role (see the session builder) and rely on `default_model` driving
///   the model — pinning `chat` here would regress them.
/// - `summarization` / `memory`, which are pinned in a dedicated branch of
///   [`make_openhuman_backend`] via [`summarization_tier_model`] (fixed at
///   `summarization-v1`) rather than here, only so the `memory` alias and the
///   role string share one resolution site. They do **not** fall through to
///   `default_model`.
///
/// `subconscious` IS pinned (to the lightweight `chat-v1` tier) even though it
/// is a background workload: the cloud subconscious tick builds via the session
/// builder with `default_model = "hint:subconscious"` (a role-routing marker, not
/// a real tier), so "inherit `default_model`" would forward that marker to the
/// backend. Pinning here resolves the managed model declaratively to `chat-v1` —
/// the cheap monitoring tier the workload wants — independent of `default_model`,
/// while [`provider_for_role`] still lets `subconscious_provider` choose the
/// provider (managed / BYOK / local).
///
/// For `vision` the default-inheritance mismatch is not just suboptimal but
/// fatal: an unset `vision_provider` would resolve to `chat-v1`,
/// `model_supports_vision` would report `false`, and the turn engine would strip
/// every attached image — leaving the managed vision sub-agent blind.
pub(super) fn managed_tier_for_role(role: &str) -> Option<&'static str> {
    use crate::config::{
        MODEL_AGENTIC_V1, MODEL_BURST_V1, MODEL_CHAT_V1, MODEL_CODING_V1, MODEL_REASONING_V1,
        MODEL_VISION_V1,
    };
    match role {
        "reasoning" => Some(MODEL_REASONING_V1),
        "agentic" => Some(MODEL_AGENTIC_V1),
        "coding" => Some(MODEL_CODING_V1),
        // Burst rides the managed backend's high-throughput tier. Pinned here
        // (rather than collapsing to `default_model`) so `hint = "burst"`
        // workers actually reach `burst-v1`.
        // There is no `burst_provider` knob: burst is managed-only.
        "burst" => Some(MODEL_BURST_V1),
        "vision" => Some(MODEL_VISION_V1),
        // Background subconscious tick/triage: pinned to the lightweight chat
        // tier (see the doc above for why it is pinned despite being background).
        "subconscious" => Some(MODEL_CHAT_V1),
        _ => None,
    }
}

/// The **managed-backend** summarization tier model — fixed at
/// [`MODEL_SUMMARIZATION_V1`] (`summarization-v1`).
///
/// Read **only** on the managed OpenHuman path (inside [`make_openhuman_backend`]),
/// so it is consumed iff the `summarization`/`memory` role actually resolves to
/// the managed backend — BYOK and local routes carry their own model in the
/// provider string and never reach here.
///
/// The managed summarization tier is intentionally **not** user-overridable: the
/// hosted backend serves exactly one tier (`summarization-v1`) for this workload,
/// so there is nothing else valid to point it at. Users who want a different
/// model run summarization on a BYOK/local `memory_provider`, where the model
/// rides in the provider string. (`memory_tree.cloud_llm_model` is no longer
/// consumed — see its config doc.)
pub(crate) fn summarization_tier_model() -> &'static str {
    crate::config::MODEL_SUMMARIZATION_V1
}

/// Build the OpenHuman backend provider (session-JWT auth).
///
/// `role` is the workload name (e.g. `"chat"`, `"coding"`, `"vision"`). A
/// specialised workload role is pinned to its canonical managed tier via
/// [`managed_tier_for_role`] so the `hint = "..."` a sub-agent declares actually
/// reaches the matching backend tier instead of collapsing to `default_model`.
/// The `summarization`/`memory` roles resolve their tier from
/// [`summarization_tier_model`] (fixed at `summarization-v1`) so they never
/// collapse to `default_model`. The generic `chat` role (and background roles)
/// keep inheriting `config.default_model`.
/// Resolve the managed OpenHuman backend for `role` — the model id (tier /
/// summarization / default, with `hint:<tier>` translation) plus a configured
/// [`OpenHumanBackendModel`]. Shared by both the `Provider` path
/// ([`make_openhuman_backend`]) and the crate `ChatModel` path
/// ([`make_openhuman_backend_model`], issue #4727 Motion B).
pub(super) fn resolve_managed_backend(
    role: &str,
    config: &Config,
) -> anyhow::Result<(OpenHumanBackendModel, String)> {
    resolve_managed_backend_with_model_override(role, config, None)
}

pub(super) fn resolve_managed_backend_with_model_override(
    role: &str,
    config: &Config,
    model_override: Option<&str>,
) -> anyhow::Result<(OpenHumanBackendModel, String)> {
    let model = if let Some(tier) = managed_tier_for_role(role) {
        log::debug!(
            "[providers][chat-factory] role={} pinned to managed tier model={}",
            role,
            tier
        );
        tier.to_string()
    } else if matches!(role, "summarization" | "memory") {
        // Managed summarization/memory tier — fixed at `summarization-v1` rather
        // than inherited from `config.default_model`, so every managed
        // summarization caller — the memory tree, the chat-turn payload
        // summarizer, meeting summaries, and any `hint = "summarization"`
        // sub-agent — reaches the dedicated tier instead of silently collapsing
        // to `chat-v1`. BYOK/local routes never reach here — they build from the
        // provider string.
        let tier = summarization_tier_model().to_string();
        log::debug!(
            "[providers][chat-factory] role={} resolved managed summarization tier model={}",
            role,
            tier
        );
        tier
    } else {
        config
            .default_model
            .clone()
            .filter(|m| !m.trim().is_empty())
            .unwrap_or_else(|| "reasoning-v1".to_string())
    };
    // Critical: pass the *config's* workspace directory through so the
    // provider's `AuthService` reads `auth-profiles.json` from the
    // same dir login wrote to. Without this, `ProviderRuntimeOptions::default()`
    // leaves `openhuman_dir = None`, the provider falls back to
    // `~/.openhuman`, and reads an unrelated (or empty)
    // profile store — surfacing as "No backend session: store a JWT
    // via auth (app-session)" even though login just succeeded in the
    // user's actual workspace (e.g. test workspaces under OPENHUMAN_WORKSPACE).
    let options = ProviderRuntimeOptions {
        openhuman_dir: config.config_path.parent().map(std::path::PathBuf::from),
        secrets_encrypt: config.secrets.encrypt,
        ..ProviderRuntimeOptions::default()
    };
    log::debug!(
        "[providers][chat-factory] building openhuman backend provider model={} state_dir={:?} secrets_encrypt={}",
        model,
        options.openhuman_dir,
        options.secrets_encrypt
    );
    // Translate `hint:<tier>` model strings into the OpenHuman backend's
    // canonical tier names.  Unrecognised `hint:*` strings (e.g. `hint:reaction`
    // for lightweight models) are forwarded as-is — the backend is authoritative
    // over which hint values it accepts, and the web-chat model_override path
    // uses these verbatim.  Only non-hint strings that are not a known canonical
    // tier (stale `default_model` values written by older UI versions, e.g.
    // "deepseek-v4-pro", "claude-opus-4-7") fall back to the platform default.
    let model = match model.strip_prefix("hint:") {
        Some("reasoning") => crate::config::MODEL_REASONING_V1.to_string(),
        Some("chat") => crate::config::MODEL_CHAT_V1.to_string(),
        Some("agentic") => crate::config::MODEL_AGENTIC_V1.to_string(),
        Some("burst") => crate::config::MODEL_BURST_V1.to_string(),
        Some("coding") => crate::config::MODEL_CODING_V1.to_string(),
        Some("summarization") => crate::config::MODEL_SUMMARIZATION_V1.to_string(),
        Some("vision") => crate::config::MODEL_VISION_V1.to_string(),
        Some(_) => {
            // Unrecognised hint — forward verbatim; the backend decides validity.
            model
        }
        None => {
            // `model` is guaranteed non-empty here: an empty/whitespace
            // `default_model` was already normalised to `reasoning-v1` above, and
            // the managed-tier / summarization branches yield non-empty tier
            // constants. So a non-`hint:` id is either a known canonical tier or a
            // raw/BYOK id the user pinned — both forward verbatim; only the log
            // line differs.
            if is_known_openhuman_tier(&model) {
                model
            } else {
                // Unrecognised NON-empty model id — a raw/BYOK model the user
                // pinned (e.g. `claude-opus-4`, written into `default_model` or
                // a per-agent model pin). Forward it verbatim so the selected
                // model actually reaches provider construction instead of the
                // core silently collapsing it onto `reasoning-v1`. The managed
                // backend is authoritative over validity and returns a clear
                // error for a genuinely bad id (issue #4598).
                log::debug!(
                    "[providers][chat-factory] forwarding raw/BYOK model '{}' verbatim to the \
                     OpenHuman backend (not a managed tier); the backend validates it",
                    model
                );
                model
            }
        }
    };
    let model = model_override
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or(model);

    // Egress spine (privacy epic S2, #4436): managed backend resolution is the
    // universal chokepoint for EVERY managed-backend inference construction —
    // the direct ChatModel path and both turn paths
    // (`create_turn_chat_model[_from_string]_with_native_tools`) resolve here.
    // Emitting once here guarantees the default managed chat turn discloses
    // egress exactly once (see `emit_inference_egress`).
    crate::security::egress::emit_external_transfer(
        crate::security::egress::EgressDescriptor::inference("openhuman", &model, true),
    );
    Ok((
        OpenHumanBackendModel::new(config.api_url.as_deref(), &options, model.clone()),
        model,
    ))
}

/// The managed OpenHuman backend as a crate-native host `ChatModel`
/// ([`OpenHumanBackendModel`], issue #4727 Motion B) — the cutover replacement
/// for the `Provider` path. Same resolution; wraps the backend so the harness
/// holds a crate `ChatModel` and the dynamic JWT + `thread_id` + billing envelope
/// are bridged onto the crate wire client per call.
pub(crate) fn make_openhuman_backend_model(
    role: &str,
    config: &Config,
) -> anyhow::Result<(
    std::sync::Arc<dyn tinyinference::model::ChatModel<()>>,
    String,
)> {
    let (model_client, model) = resolve_managed_backend(role, config)?;
    let chat: std::sync::Arc<dyn tinyinference::model::ChatModel<()>> =
        std::sync::Arc::new(model_client);
    Ok((chat, model))
}
