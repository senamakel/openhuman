//! Product policy for module-owned x402 spending limits.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Spending limits passed to the TinyWallet x402 payment service.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct X402Config {
    /// Maximum atomic units allowed for one payment request.
    #[serde(default = "default_per_request_max_atomic")]
    pub per_request_max_atomic: u64,
    /// Maximum atomic units allowed per day.
    #[serde(default = "default_daily_max_atomic")]
    pub daily_max_atomic: u64,
    /// Maximum atomic units allowed per month.
    #[serde(default = "default_monthly_max_atomic")]
    pub monthly_max_atomic: u64,
}

impl Default for X402Config {
    fn default() -> Self {
        Self {
            per_request_max_atomic: default_per_request_max_atomic(),
            daily_max_atomic: default_daily_max_atomic(),
            monthly_max_atomic: default_monthly_max_atomic(),
        }
    }
}

fn default_per_request_max_atomic() -> u64 {
    1_000_000
}

fn default_daily_max_atomic() -> u64 {
    10_000_000
}

fn default_monthly_max_atomic() -> u64 {
    100_000_000
}
