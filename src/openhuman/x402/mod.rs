//! x402 — HTTP 402 payment protocol for machine-payable APIs.
//!
//! Intercepts HTTP 402 responses carrying a `PAYMENT-REQUIRED` header,
//! constructs a Solana SPL token payment (typically USDC), signs it with the
//! wallet's ed25519 key, and retries the request with the payment proof in a
//! `PAYMENT-SIGNATURE` header. The facilitator co-signs as fee payer and
//! broadcasts, so the client never needs SOL for gas.
//!
//! Protocol spec: <https://x402.org> / coinbase/x402 (v2).

mod ops;
pub(crate) mod store;
mod types;

#[cfg(test)]
mod x402_tests;

pub use ops::{handle_402, try_paid_request, X402Client};
pub use types::{
    PaymentPayload, PaymentRequired, PaymentRequirements, ResourceInfo, SettlementResponse,
};
