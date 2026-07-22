//! Contract gate for late-bound Composio actions (#4853).
//!
//! Per-action Composio tools handed to `integrations_agent` are built from
//! the lightweight `list_tools` response — a one-line description with a
//! parameter schema that is often thin or absent (see
//! `fetch_toolkit_actions`, consumed in the
//! sub-agent runner). The model therefore composes calls before the action's
//! FULL contract is in context and guesses argument formats — most visibly, it
//! sends Gmail `query` strings without the quoting Gmail search syntax requires,
//! so `GMAIL_FETCH_EMAILS` returns zero results.
//!
//! The gate makes the full contract enter context BEFORE execution: on the
//! first call to an action this turn, if a fuller live contract is available
//! (via the cached `fetch_live_toolkit_catalog`), it is returned as a
//! recoverable tool error instead of executing. The retry — now with the
//! schema/description in context — proceeds normally. This mirrors the
//! discover-then-call discipline the generic `composio_execute` dispatcher
//! already expects (`composio_list_tools` → `composio_execute`), but enforces
//! it on the per-action surface where the model never sees the full schema.
//!
//! Scope: this pass gates the per-action Composio surface
//! ([`super::action_tool::ComposioActionTool`]). Generalising the same gate to
//! the `composio_execute` dispatcher, the MCP bridges, and the Workflow
//! dispatchers — plus resetting the per-turn state on context compaction — is
//! tracked as follow-up (a shared `ToolMiddleware` at the turn-harness seam is
//! the natural home; see the PR description).

use std::collections::HashSet;
use std::sync::Mutex;

use crate::openhuman::config::Config;
// The live-contract lookup is sourced from the flows/tinyflows caps catalog,
// which is compiled out when the `flows` feature is off (#4912). The gate then
// simply has no fuller contract to surface and always proceeds, so the import
// and the lookup/format helpers below are gated in lockstep.
#[cfg(feature = "flows")]
use crate::openhuman::composio::providers::toolkit_from_slug;
#[cfg(feature = "flows")]
use crate::openhuman::tinyflows::caps::{fetch_live_toolkit_catalog, ToolContract};

/// Record of which action contracts have already been surfaced to the model,
/// so the gate blocks a given action at most once per gate instance.
///
/// One [`ContractGate`] is held per [`super::action_tool::ComposioActionTool`]
/// instance; those tools are constructed fresh per `integrations_agent` spawn
/// and live for that spawn's tool loop. That loop is a single agent turn in the
/// common case, so "seen" behaves as per-turn state without any task-local
/// plumbing — but a long-lived spawn can span multiple turns, and this gate
/// does NOT reset when the surfaced schema drops out of context via compaction
/// (tracked as follow-up; see the module-level note). Interior-mutable so the
/// gate can record state through the tool's `&self` `execute`.
#[derive(Default)]
pub struct ContractGate {
    seen: Mutex<HashSet<String>>,
}

impl ContractGate {
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert `slug` (normalised to upper-case) into the seen-set. Returns
    /// `true` when it was NOT already present — i.e. this is the first time the
    /// gate has been consulted for this action for this gate's lifetime.
    ///
    /// The lock is taken and released entirely within this call, so no guard is
    /// held across the caller's later `await`.
    fn mark_seen(&self, slug: &str) -> bool {
        let mut guard = self
            .seen
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.insert(slug.to_ascii_uppercase())
    }
}

/// Outcome of consulting the gate for one action call.
pub enum GateDecision {
    /// Return this text to the model as a recoverable tool error; the model
    /// retries with the contract in context.
    Surface(String),
    /// Execute the action normally.
    Proceed,
}

/// Consult the gate before executing `action_slug` with the model's `args`.
///
/// On the FIRST consult for a slug this turn, if a fuller live contract can be
/// resolved, the gate compares the model's supplied `args` against it:
///
/// - **Args already satisfy the contract** (all required present, every supplied
///   key a known property, types compatible) → [`GateDecision::Proceed`]. The
///   model did not need the schema, so bouncing would be pure overhead — and, on
///   the weak text-mode `integrations_agent` path, forcing a needless retry lets
///   a Kimi-family model corrupt the re-issued call (`<|"|>` sentinel-token leak)
///   and loop forever without ever executing (#5119).
/// - **Args do NOT satisfy the contract** (missing required, unknown key, wrong
///   type — i.e. the model *guessed*) → [`GateDecision::Surface`] with the
///   formatted contract, exactly the case the gate exists for (#4853).
///
/// The slug is marked seen on this first consult either way, so every later
/// consult — and any consult where no live contract is available (unconfigured
/// client, unknown action, network miss) — returns [`GateDecision::Proceed`]:
/// the gate never blocks an action more than once and never blocks when it
/// cannot help.
pub async fn consult(
    gate: &ContractGate,
    config: &Config,
    action_slug: &str,
    args: &serde_json::Value,
) -> GateDecision {
    // Mark first (releasing the lock) so the retry — and any concurrent
    // sibling call — proceeds even if the contract lookup below is slow.
    let first_time = gate.mark_seen(action_slug);
    if !first_time {
        tracing::debug!(
            target: "composio",
            slug = %action_slug,
            "[composio][contract-gate] contract already surfaced this turn; proceeding"
        );
        return GateDecision::Proceed;
    }

    // The live catalog lives in the flows/tinyflows caps layer. With `flows`
    // compiled out there is no catalog source, so the gate can never surface a
    // fuller contract and always proceeds (the per-action tool still runs; it
    // just does not get the pre-execute contract nudge).
    #[cfg(feature = "flows")]
    if let Some(contract) = lookup_contract(config, action_slug).await {
        // Validate-then-pass (#5119): only surface when the model actually needs
        // the schema. A call whose args already conform is executed directly.
        if args_satisfy_contract(args, &contract) {
            tracing::debug!(
                target: "composio",
                slug = %action_slug,
                "[composio][contract-gate] args already satisfy the live contract; proceeding without surfacing"
            );
            return GateDecision::Proceed;
        }
        tracing::debug!(
            target: "composio",
            slug = %action_slug,
            has_input_schema = contract.input_schema.is_some(),
            required_arg_count = contract.required_args.len(),
            "[composio][contract-gate] surfacing full contract before first execute"
        );
        return GateDecision::Surface(format_contract(action_slug, &contract));
    }

    // `config` and `args` are only consulted through the flows-gated lookup +
    // validation above.
    #[cfg(not(feature = "flows"))]
    let _ = (config, args);

    tracing::debug!(
        target: "composio",
        slug = %action_slug,
        "[composio][contract-gate] no live contract available; proceeding without gating"
    );
    GateDecision::Proceed
}

/// Whether the model's supplied `args` already conform to `contract` — the test
/// that lets the gate execute a well-formed first call instead of bouncing it
/// (#5119). Conservative: an object whose required args are all present, whose
/// every supplied key is a known schema property, and whose values are
/// type-compatible with the schema. Anything short of that is treated as a
/// guess and surfaces the contract (#4853).
///
/// Type checks are intentionally lenient about stringified scalars (a model may
/// send `max_results: "10"`), so only a genuinely wrong shape — a string where
/// an array is required, an unknown/invented key, a missing required arg — fails.
/// When the schema publishes no `properties`, only the required-args presence
/// check applies.
#[cfg(feature = "flows")]
fn args_satisfy_contract(args: &serde_json::Value, contract: &ToolContract) -> bool {
    let obj = match args.as_object() {
        Some(obj) => obj,
        // Non-object args satisfy the contract only when nothing is required
        // (e.g. a no-arg action called with `null`/absent args).
        None => return contract.required_args.is_empty(),
    };

    // Every required argument must be present and non-null.
    for req in &contract.required_args {
        match obj.get(req) {
            Some(v) if !v.is_null() => {}
            _ => return false,
        }
    }

    // If the schema publishes its properties, every supplied key must be known
    // (no invented args) and type-compatible. A hallucinated key or a
    // wrong-typed value is exactly the guess the gate exists to catch.
    if let Some(props) = contract
        .input_schema
        .as_ref()
        .and_then(|s| s.get("properties"))
        .and_then(|p| p.as_object())
    {
        for (key, value) in obj {
            // `connection_id` is an OpenHuman-injected routing parameter
            // (`ComposioActionTool::parameters_schema` / `ComposioExecuteTool`),
            // consumed before dispatch and absent from Composio's live catalog
            // `input_schema`. Skip it so a valid multi-account call isn't bounced
            // as an "unknown key" into the retry path this gate exists to avoid.
            if key == "connection_id" {
                continue;
            }
            match props.get(key) {
                None => return false,
                Some(prop) => {
                    if let Some(expected) = prop.get("type").and_then(|t| t.as_str()) {
                        if !json_value_matches_type(value, expected) {
                            return false;
                        }
                    }
                }
            }
        }
    }

    true
}

/// Loose JSON-Schema scalar/compound `type` check used by
/// [`args_satisfy_contract`]. Numeric/boolean types also accept a string that
/// parses to that type, so a model sending `"10"` for an `integer` field is not
/// treated as a schema violation. An unrecognised or union `type` (the
/// `and_then(as_str)` returns `None` for a `["string","null"]` array) is never
/// reached here, so callers simply skip the check — lenient by construction.
#[cfg(feature = "flows")]
fn json_value_matches_type(value: &serde_json::Value, expected: &str) -> bool {
    match expected {
        "string" => value.is_string(),
        "integer" => {
            value.is_i64()
                || value.is_u64()
                || value
                    .as_str()
                    .is_some_and(|s| s.trim().parse::<i64>().is_ok())
        }
        "number" => {
            value.is_number()
                || value
                    .as_str()
                    .is_some_and(|s| s.trim().parse::<f64>().is_ok())
        }
        "boolean" => {
            value.is_boolean()
                || value
                    .as_str()
                    .is_some_and(|s| matches!(s.trim(), "true" | "false"))
        }
        "array" => value.is_array(),
        "object" => value.is_object(),
        "null" => value.is_null(),
        // Unknown/unsupported type keyword → don't reject on type grounds.
        _ => true,
    }
}

/// Resolve the full live contract for `action_slug` from the process-cached
/// live toolkit catalog. Returns `None` when the toolkit can't be derived, the
/// catalog can't be fetched (unconfigured / offline — `fetch_live_toolkit_catalog`
/// degrades to `None`), or the action isn't in it.
#[cfg(feature = "flows")]
async fn lookup_contract(config: &Config, action_slug: &str) -> Option<ToolContract> {
    let toolkit = toolkit_from_slug(action_slug)?;
    let contracts = fetch_live_toolkit_catalog(config, &toolkit).await?;
    contracts
        .into_iter()
        .find(|c| c.slug.eq_ignore_ascii_case(action_slug))
}

/// Render the contract into a compact instruction for the model. Contains only
/// the provider's own action description + JSON schema — no user data / PII.
#[cfg(feature = "flows")]
fn format_contract(action_slug: &str, contract: &ToolContract) -> String {
    let mut out = format!(
        "Before running `{action_slug}`, read its full contract below and then re-issue \
         the call with arguments that match it exactly.\n\n"
    );

    if let Some(desc) = contract
        .description
        .as_deref()
        .map(str::trim)
        .filter(|d| !d.is_empty())
    {
        out.push_str("Description:\n");
        out.push_str(desc);
        out.push_str("\n\n");
    }

    match contract.input_schema.as_ref() {
        Some(schema) => {
            let pretty =
                serde_json::to_string_pretty(schema).unwrap_or_else(|_| schema.to_string());
            out.push_str("Input JSON schema:\n");
            out.push_str(&pretty);
            out.push('\n');
        }
        None => out.push_str("Input JSON schema: not published by the provider for this action.\n"),
    }

    if !contract.required_args.is_empty() {
        out.push_str(&format!(
            "\nRequired arguments: {}\n",
            contract.required_args.join(", ")
        ));
    }

    out.push_str(
        "\nCompose every argument to match this schema and any format rules in the \
         description. Text-search fields in particular often require the provider's exact \
         query syntax (for example, Gmail needs multi-word phrases quoted, like \
         subject:\"quarterly report\"). Then call the action again with the corrected \
         arguments.",
    );
    out
}

// The gate's unit tests seed the flows/tinyflows live-catalog cache, so they
// only compile and run with the `flows` feature on.
#[cfg(all(test, feature = "flows"))]
#[path = "contract_gate_tests.rs"]
mod tests;
