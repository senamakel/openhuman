//! Per-call and per-turn cost accounting for an agent's tool-call loop.
//!
//! A call's cost is one of three things, never a guess:
//!
//! - **Charged**: the amount the provider actually billed, reported back on
//!   the response (`openhuman.billing.charged_amount_usd` from the managed
//!   backend, `total_cost_usd` from a CLI provider). It may be zero for a
//!   free route; that is still a known cost.
//! - **Estimated**: no charge was reported, but the model is in the vendor
//!   price catalog ([`crate::platform::cost::catalog`]), so its published
//!   list rates price the token counts. Reported as an estimate.
//! - **Unknown**: no charge and no catalogued price. Nothing is reported for
//!   it; a turn with any unknown call has an unknown total.
//!
//! There is deliberately no default rate for an unrecognised model. One used
//! to exist ($3 / $0.30 / $15 per MTok) and priced a glm-5.3-flash thread at
//! $4.25 when the provider billed about $0.30.

use crate::inference::provider::BilledUsage;

/// Per-million-token rates for a single model.
///
/// All prices are USD per million tokens. `cached_input_per_mtok_usd`
/// applies to the `cached_input_tokens` portion of the usage block (KV
/// prefix cache hits on supporting backends); the remaining
/// `input_tokens - cached_input_tokens` are charged at
/// `input_per_mtok_usd`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ModelPricing {
    /// Model id, e.g. `"claude-sonnet-4-6"`.
    pub(crate) model: &'static str,
    /// Standard prompt rate, USD per million input tokens.
    pub(crate) input_per_mtok_usd: f64,
    /// Cached-prefix prompt rate, USD per million cached input tokens.
    pub(crate) cached_input_per_mtok_usd: f64,
    /// Completion rate, USD per million output tokens.
    pub(crate) output_per_mtok_usd: f64,
}

/// Published list rates for `model`, or `None` when the vendor catalog does
/// not know it.
/// Whether a model names an explicitly free OpenRouter variant.
pub(crate) fn is_free_model(model: &str) -> bool {
    model.trim().to_ascii_lowercase().ends_with(":free")
}

pub(crate) fn lookup_pricing(model: &str) -> Option<ModelPricing> {
    if is_free_model(model) {
        return Some(ModelPricing {
            model: "<free>",
            input_per_mtok_usd: 0.0,
            cached_input_per_mtok_usd: 0.0,
            output_per_mtok_usd: 0.0,
        });
    }
    let price = crate::platform::cost::catalog::lookup(model.trim())?;
    Some(ModelPricing {
        model: price.model_id,
        input_per_mtok_usd: price.input_per_mtok_usd,
        cached_input_per_mtok_usd: price.cached_input_per_mtok_usd,
        output_per_mtok_usd: price.output_per_mtok_usd,
    })
}

/// List-price estimate of one call, or `None` when the model is not
/// catalogued. Used only when the provider reported no charge.
pub fn estimate_call_cost_usd(model: &str, usage: &BilledUsage) -> Option<f64> {
    let pricing = lookup_pricing(model)?;
    let cached = usage.cached_input_tokens().min(usage.input_tokens);
    let standard_input = usage.input_tokens.saturating_sub(cached);
    let m = 1_000_000.0_f64;
    Some(
        (standard_input as f64) / m * pricing.input_per_mtok_usd
            + (cached as f64) / m * pricing.cached_input_per_mtok_usd
            + (usage.output_tokens as f64) / m * pricing.output_per_mtok_usd,
    )
}

/// Where a cost figure came from. Ordered from most to least certain, so the
/// cost of several calls takes the least certain of their sources.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Default,
    serde::Serialize,
    serde::Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum CostSource {
    /// Every call reported the amount the provider billed.
    #[default]
    Charged,
    /// At least one call was priced from the catalog's list rates.
    Estimated,
    /// At least one call had neither a charge nor a catalogued price, so the
    /// total is not known.
    Unknown,
}

/// The cost of one provider call.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CallCost {
    /// Billed by the provider (may be zero).
    Charged(f64),
    /// Catalog list-price estimate.
    Estimated(f64),
    /// Neither available.
    Unknown,
}

impl CallCost {
    /// The USD figure, or `None` when unknown.
    pub fn usd(self) -> Option<f64> {
        match self {
            Self::Charged(usd) | Self::Estimated(usd) => Some(usd),
            Self::Unknown => None,
        }
    }

    /// The source this cost contributes to a total.
    pub fn source(self) -> CostSource {
        match self {
            Self::Charged(_) => CostSource::Charged,
            Self::Estimated(_) => CostSource::Estimated,
            Self::Unknown => CostSource::Unknown,
        }
    }
}

/// The cost of one call: the provider's charge when it reported one, else
/// the catalog estimate, else unknown.
pub fn call_cost(model: &str, usage: &BilledUsage) -> CallCost {
    if usage.charge_reported && !usage.cost_is_estimate {
        return CallCost::Charged(usage.charged_amount_usd);
    }
    match estimate_call_cost_usd(model, usage) {
        Some(usd) => CallCost::Estimated(usd),
        None => CallCost::Unknown,
    }
}

/// A sum of call costs that remembers how certain it is.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct CostTally {
    /// Sum of the known call costs. Meaningless on its own once `source` is
    /// [`CostSource::Unknown`]; read it through [`CostTally::usd`].
    pub known_usd: f64,
    /// The least certain source among the calls added.
    pub source: CostSource,
}

impl CostTally {
    /// Fold one call in.
    pub fn add(&mut self, cost: CallCost) {
        self.known_usd += cost.usd().unwrap_or(0.0);
        self.source = self.source.max(cost.source());
    }

    /// Fold another tally in (a sub-step's or a child's).
    pub fn merge(&mut self, other: CostTally) {
        self.known_usd += other.known_usd;
        self.source = self.source.max(other.source);
    }

    /// The total, or `None` when any call's cost was unknown.
    pub fn usd(&self) -> Option<f64> {
        (self.source != CostSource::Unknown).then_some(self.known_usd)
    }
}

/// Running cost / token tally across every provider call inside a
/// single turn of the tool-call loop.
#[derive(Debug, Clone, Default)]
pub struct TurnCost {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cached_input_tokens: u64,
    pub cost: CostTally,
    pub call_count: u32,
}

impl TurnCost {
    /// New empty accumulator.
    pub fn new() -> Self {
        Self::default()
    }

    /// Fold a single provider call's usage into the running totals.
    pub fn add_call(&mut self, model: &str, usage: &BilledUsage) {
        self.input_tokens = self.input_tokens.saturating_add(usage.input_tokens);
        self.output_tokens = self.output_tokens.saturating_add(usage.output_tokens);
        self.cached_input_tokens = self
            .cached_input_tokens
            .saturating_add(usage.cached_input_tokens());
        self.cost.add(call_cost(model, usage));
        self.call_count = self.call_count.saturating_add(1);
    }

    /// The turn's cost so far, or `None` when any call's cost is unknown.
    pub fn total_usd(&self) -> Option<f64> {
        self.cost.usd()
    }
}

#[cfg(test)]
#[path = "cost_tests.rs"]
mod tests;
