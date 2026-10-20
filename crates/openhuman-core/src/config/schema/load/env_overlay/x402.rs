//! Legacy environment overrides for x402 spending policy.

use crate::config::schema::load::env::EnvLookup;
use crate::config::schema::Config;

impl Config {
    pub(super) fn apply_x402_env<E: EnvLookup + ?Sized>(&mut self, env: &E) {
        if let Some(value) = env
            .get("OPENHUMAN_X402_PER_REQUEST_MAX")
            .and_then(|raw| raw.parse().ok())
        {
            self.x402.per_request_max_atomic = value;
        }
        if let Some(value) = env
            .get("OPENHUMAN_X402_DAILY_MAX")
            .and_then(|raw| raw.parse().ok())
        {
            self.x402.daily_max_atomic = value;
        }
        if let Some(value) = env
            .get("OPENHUMAN_X402_MONTHLY_MAX")
            .and_then(|raw| raw.parse().ok())
        {
            self.x402.monthly_max_atomic = value;
        }
    }
}
