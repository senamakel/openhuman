//! Wire types for the x402 protocol (v2).
//!
//! All header payloads are standard-base64-encoded JSON. Network identifiers
//! use CAIP-2 format (e.g. `solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp`).

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

pub const X402_VERSION: u8 = 2;

pub const HEADER_PAYMENT_REQUIRED: &str = "PAYMENT-REQUIRED";
pub const HEADER_PAYMENT_REQUIRED_V1: &str = "X-PAYMENT-REQUIRED";
pub const HEADER_PAYMENT_SIGNATURE: &str = "PAYMENT-SIGNATURE";
pub const HEADER_PAYMENT_SIGNATURE_V1: &str = "X-PAYMENT";
pub const HEADER_PAYMENT_RESPONSE: &str = "PAYMENT-RESPONSE";

pub const SOLANA_MAINNET_CAIP2: &str = "solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp";
pub const SOLANA_DEVNET_CAIP2: &str = "solana:EtWTRABZaYq6iMfeYKouRu166VU2xqa1";

pub const USDC_MINT_MAINNET: &str = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
pub const USDC_MINT_DEVNET: &str = "4zMMC9srt5Ri5X14GAgXhaHii3GnPAEERYPJgZJDncDU";

pub const SPL_TOKEN_PROGRAM: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
pub const SPL_MEMO_PROGRAM: &str = "MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr";
pub const COMPUTE_BUDGET_PROGRAM: &str = "ComputeBudget111111111111111111111111111111";

// ---------------------------------------------------------------------------
// 402 challenge — server → client (PAYMENT-REQUIRED header)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentRequired {
    pub x402_version: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub resource: ResourceInfo,
    pub accepts: Vec<PaymentRequirements>,
    #[serde(default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extensions: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceInfo {
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentRequirements {
    pub scheme: String,
    pub network: String,
    /// Amount in atomic token units (e.g. 1 USDC = 1_000_000).
    pub amount: String,
    /// Token mint address (Solana) or contract address (EVM).
    pub asset: String,
    /// Recipient wallet address.
    pub pay_to: String,
    pub max_timeout_seconds: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extra: Option<PaymentExtra>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentExtra {
    /// Facilitator pubkey that will co-sign as fee payer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fee_payer: Option<String>,
    /// Required memo value for transaction uniqueness.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memo: Option<String>,
}

// ---------------------------------------------------------------------------
// Payment proof — client → server (PAYMENT-SIGNATURE header)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentPayload {
    pub x402_version: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource: Option<ResourceInfo>,
    pub accepted: PaymentRequirements,
    pub payload: SolanaPaymentProof,
    #[serde(default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extensions: serde_json::Map<String, serde_json::Value>,
}

/// Solana `exact` scheme payload — a partially-signed `VersionedTransaction`
/// serialized as standard base64. The facilitator adds its fee-payer signature
/// and broadcasts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SolanaPaymentProof {
    pub transaction: String,
}

// ---------------------------------------------------------------------------
// Settlement response — server → client (PAYMENT-RESPONSE header)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettlementResponse {
    pub success: bool,
    /// Base58 transaction signature (Solana) or hex tx hash (EVM).
    pub transaction: String,
    pub network: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub amount: Option<String>,
    #[serde(default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extensions: serde_json::Map<String, serde_json::Value>,
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

impl PaymentRequired {
    /// Find the first `accepts` entry whose network starts with `"solana:"` and
    /// whose scheme is `"exact"`.
    pub fn solana_exact_requirement(&self) -> Option<&PaymentRequirements> {
        self.accepts
            .iter()
            .find(|r| r.scheme == "exact" && r.network.starts_with("solana:"))
    }
}

impl PaymentRequirements {
    pub fn is_solana_mainnet(&self) -> bool {
        self.network == SOLANA_MAINNET_CAIP2
    }

    pub fn fee_payer_pubkey(&self) -> Option<&str> {
        self.extra.as_ref()?.fee_payer.as_deref()
    }

    pub fn memo_value(&self) -> Option<&str> {
        self.extra.as_ref()?.memo.as_deref()
    }
}
