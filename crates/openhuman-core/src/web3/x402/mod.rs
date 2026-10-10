//! x402 — HTTP 402 payment protocol for machine-payable APIs.
//!
//! The protocol lives in the `tinywallet-x402` crate (`vendor/tinywallet`): the
//! 402 client, the EVM EIP-3009 and Solana SPL payment builders, the spending
//! ledger and the `x402_request` agent tool. This module is the host side of
//! it — what OpenHuman adds and nothing more:
//!
//! - [`seams`] — the wallet ([`seams::WalletPaymentSigner`]: keyring secret,
//!   decrypt, the wallet module) and outbound-proxy policy
//!   ([`seams::RuntimeProxyPolicy`]) the crate is handed. The mnemonic never
//!   enters the crate.
//! - `schemas` — the `x402_*` RPC controllers (namespace strings are wire
//!   contracts).
//! - [`budget`] — where the spending limits come from (defaults plus the
//!   `OPENHUMAN_X402_*` environment overrides).
//! - this file — a facade re-exporting the crate so existing callers
//!   (`http_request`, boot, the tool registry) keep their paths.
//!
//! Protocol spec: <https://x402.org> / coinbase/x402 (v2).
//!
//! ## Compile-time gate (`web3` feature)
//!
//! `pub mod x402;` is ALWAYS compiled — it is a facade. The real payment
//! machinery is gated behind the default-ON `web3` Cargo feature (shared with
//! `openhuman::web3::wallet` + `openhuman::web3`). When the feature is off, [`stub`]
//! takes its place and exposes the always-on entry points (`init_ledger`,
//! `all_x402_registered_controllers`, `all_x402_controller_schemas`) with
//! no-op / empty bodies. The `X402RequestTool` and the http_request 402-retry
//! path are `#[cfg(feature = "web3")]` at their call sites, so the rest of the
//! payment surface (`PaymentRecord`, `store`, `SettlementResponse`, …) is not
//! referenced when off and need not be stubbed.

#[cfg(feature = "web3")]
pub(crate) mod budget;
#[cfg(feature = "web3")]
mod records;
#[cfg(feature = "web3")]
mod schemas;
#[cfg(feature = "web3")]
pub(crate) mod seams;

#[cfg(feature = "web3")]
pub use records::pending_record;
#[cfg(feature = "web3")]
pub use schemas::all_controller_schemas as all_x402_controller_schemas;
#[cfg(feature = "web3")]
pub use schemas::all_registered_controllers as all_x402_registered_controllers;
#[cfg(feature = "web3")]
pub use tinywallet_x402::ledger::{PaymentRecord, PaymentStatus, SpendingBudget};
#[cfg(feature = "web3")]
pub use tinywallet_x402::protocol::{handle_402, X402Client, X402Error, X402PaymentResult};
#[cfg(feature = "web3")]
pub use tinywallet_x402::wire::{
    EvmAuthorization, EvmPaymentProof, PaymentChain, PaymentPayload, PaymentProof, PaymentRequired,
    PaymentRequirements, ResourceInfo, SettlementResponse, SolanaPaymentProof,
};

/// The process-wide spending ledger, as the crate exposes it.
#[cfg(feature = "web3")]
pub(crate) mod store {
    pub use tinywallet_x402::ledger::{with_ledger, with_ledger_mut};
}

/// Open the spending ledger under `workspace_dir` for this process.
///
/// The limits are [`budget::budget_from_env`]: the crate's defaults with any
/// `OPENHUMAN_X402_*` override applied.
#[cfg(feature = "web3")]
pub fn init_ledger(workspace_dir: &std::path::Path, session_id: &str) {
    tinywallet_x402::ledger::init_global(workspace_dir, session_id, budget::budget_from_env());
}

/// End-to-end 402 handler for the HTTP tool layer: parse the challenge, check
/// the budget, and have the wallet build and sign the payment.
#[cfg(feature = "web3")]
pub async fn handle_402_and_pay(
    response_headers: &reqwest::header::HeaderMap,
    request_url: &str,
) -> Result<X402PaymentResult, X402Error> {
    tinywallet_x402::protocol::handle_402_and_pay(&seams::payments(), response_headers, request_url)
        .await
}

/// The `x402_request` agent tool, wired to OpenHuman's wallet, chain
/// transport, proxy and network policy. `allowed_domains` is the same list
/// passed to the other agent network tools.
#[cfg(feature = "web3")]
pub fn request_tool(
    security: std::sync::Arc<crate::security::SecurityPolicy>,
    allowed_domains: Vec<String>,
) -> tinywallet_x402::tools::X402RequestTool {
    seams::request_tool(security, allowed_domains)
}

// ---------------------------------------------------------------------------
// Disabled facade — compiled only when the `web3` feature is OFF.
// ---------------------------------------------------------------------------

#[cfg(not(feature = "web3"))]
mod stub;
#[cfg(not(feature = "web3"))]
pub use stub::*;
