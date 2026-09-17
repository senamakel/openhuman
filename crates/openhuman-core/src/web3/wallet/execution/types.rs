//! Wire types for the wallet execution surface: chain/asset/balance
//! snapshots, the prepare→execute quote lifecycle, transaction lookups, and
//! the RPC param shapes.

use serde::{Deserialize, Serialize};

use super::super::defaults::EvmNetwork;
use super::super::ops::WalletChain;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChainStatus {
    pub chain: WalletChain,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evm_network: Option<EvmNetwork>,
    pub configured: bool,
    pub provider_status: ProviderStatus,
    pub rpc_url: String,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProviderStatus {
    Ready,
    Missing,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SupportedAsset {
    pub chain: WalletChain,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evm_network: Option<EvmNetwork>,
    pub symbol: String,
    pub name: String,
    pub native: bool,
    pub decimals: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contract_address: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BalanceInfo {
    pub chain: WalletChain,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evm_network: Option<EvmNetwork>,
    pub address: String,
    pub asset_symbol: String,
    pub decimals: u8,
    pub raw: String,
    pub formatted: String,
    pub provider_status: ProviderStatus,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PreparedKind {
    NativeTransfer,
    TokenTransfer,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PreparedStatus {
    AwaitingConfirmation,
    Broadcasted,
    Consumed,
}

/// Identity of the chat thread that prepared a quote.
///
/// The wallet executes prepare/execute as a two-step flow keyed by `quote_id`.
/// `quote_id`s are visible in the shared chat broadcast (the prepared-tx
/// summary that gets sent back into the channel), so a co-channel caller can
/// read another caller's `quote_id` and try to drive its execute from their
/// own (now per-sender-isolated, post-#2331) agent session. Binding the
/// quote to the originating chat thread closes that gap: execute is only
/// allowed when the caller's `current_owner()` equals the prepare-time owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct QuoteOwner {
    pub(crate) thread_id: String,
    pub(crate) client_id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreparedTransaction {
    pub quote_id: String,
    pub kind: PreparedKind,
    pub chain: WalletChain,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evm_network: Option<EvmNetwork>,
    pub from_address: String,
    pub to_address: String,
    pub asset_symbol: String,
    pub amount_raw: String,
    pub amount_formatted: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub receive_symbol: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_receive_raw: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub calldata: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_address: Option<String>,
    pub estimated_fee_raw: String,
    pub status: PreparedStatus,
    pub created_at_ms: u64,
    pub expires_at_ms: u64,
    pub notes: Vec<String>,
    /// Chat-thread owner stamped at prepare time. Present when the quote
    /// was prepared from inside an interactive chat turn (web channel sets
    /// `APPROVAL_CHAT_CONTEXT`); `None` for CLI / direct-RPC / background
    /// callers. Internal gate data — never serialized over the wire.
    #[serde(skip_serializing)]
    pub(crate) owner: Option<QuoteOwner>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionResult {
    pub quote_id: String,
    pub status: PreparedStatus,
    pub chain: WalletChain,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evm_network: Option<EvmNetwork>,
    pub transaction_hash: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub explorer_url: Option<String>,
    pub transaction: PreparedTransaction,
}

/// Result of a low-level "sign this unsigned transaction and broadcast it"
/// primitive. Unlike [`ExecutionResult`], this carries no `PreparedTransaction`
/// — it is the minimal output the `web3` layer needs after handing the wallet
/// an externally-built (e.g. deBridge) unsigned transaction to sign+broadcast.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RawBroadcastResult {
    pub transaction_hash: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub explorer_url: Option<String>,
    /// Simulated fee in the chain's smallest unit. `None` when the fee is not
    /// known at broadcast time (e.g. Solana's dynamic base+priority fee, which
    /// must be read back from the confirmed transaction).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fee_raw: Option<String>,
}

/// Normalized lifecycle state of a broadcast transaction.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TxState {
    /// Seen by the node but not yet included in a block.
    Pending,
    /// Included in a block and succeeded.
    Confirmed,
    /// Included in a block but reverted/failed.
    Failed,
    /// The node has no record of this hash.
    NotFound,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TxStatusInfo {
    pub chain: WalletChain,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evm_network: Option<EvmNetwork>,
    pub hash: String,
    pub state: TxState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confirmations: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block_number: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TxReceiptInfo {
    pub chain: WalletChain,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evm_network: Option<EvmNetwork>,
    pub hash: String,
    pub found: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub success: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block_number: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gas_used: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fee_raw: Option<String>,
    /// Raw provider receipt payload, passed through unchanged.
    pub raw: serde_json::Value,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TxLookupInfo {
    pub chain: WalletChain,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evm_network: Option<EvmNetwork>,
    pub hash: String,
    pub found: bool,
    /// Raw provider transaction payload, passed through unchanged.
    pub raw: serde_json::Value,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrepareTransferParams {
    pub chain: WalletChain,
    pub to_address: String,
    pub amount_raw: String,
    #[serde(default)]
    pub asset_symbol: Option<String>,
    #[serde(default)]
    pub evm_network: Option<EvmNetwork>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutePreparedParams {
    pub quote_id: String,
    pub confirmed: bool,
}
