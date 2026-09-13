//! Wallet execution surface — read tools (balances / supported assets /
//! network defaults / chain status) and write tools (prepare-then-execute)
//! for native sends, token transfers, swaps, and contract calls.
//!
//! Execution is intentionally narrower than the metadata surface:
//! - Every write must be prepared first, then explicitly confirmed.
//! - Secret material stays encrypted at rest in core-owned storage.
//! - EVM (Ethereum + Base/Arbitrum/Optimism/Polygon L2s), Bitcoin (P2WPKH),
//!   Solana (native + SPL), and Tron (native + TRC20) all sign and broadcast.
//!   Swap broadcast is still quote-only on every chain.
//!
//! ## Module layout
//!
//! - [`types`] — wire types: chain/asset/balance snapshots, the quote
//!   lifecycle, transaction lookups, and RPC param shapes.
//! - [`quotes`] — the prepare→execute quote store and its chat-thread
//!   ownership check.
//! - [`validate`] — address/amount/calldata validation, formatting, and hex
//!   conversions.
//! - [`accounts`] — resolving a derived wallet account for a chain.
//! - [`queries`] — the read-only surface: defaults, supported assets, chain
//!   status, balances.
//! - [`transfer`] — preparing a transfer quote.
//! - [`tx_lookup`] — transaction status / receipt / raw lookup.
//! - [`broadcast`] — signing, broadcasting, and `execute_prepared`.

mod accounts;
mod broadcast;
mod queries;
mod quotes;
mod transfer;
mod tx_lookup;
mod types;
mod validate;

pub(crate) use accounts::require_evm_account;
pub use broadcast::execute_prepared;
pub(crate) use broadcast::{sign_and_broadcast_evm, sign_and_broadcast_solana};
pub use queries::{balances, chain_status, network_defaults, supported_assets};
pub use quotes::prepared_quotes_for_test;
#[cfg(test)]
pub(crate) use quotes::{insert_quote_for_test, reset_quote_store_for_tests};
pub use transfer::prepare_transfer;
pub use tx_lookup::{lookup_tx, tx_receipt, tx_status};
pub(crate) use types::RawBroadcastResult;
pub use types::{
    BalanceInfo, ChainStatus, ExecutePreparedParams, ExecutionResult, PrepareTransferParams,
    PreparedKind, PreparedStatus, PreparedTransaction, ProviderStatus, SupportedAsset,
    TxLookupInfo, TxReceiptInfo, TxState, TxStatusInfo,
};
#[cfg(test)]
pub(crate) use validate::compressed_public_key;
pub(crate) use validate::validate_calldata;
pub use validate::{hex_to_u128, u128_to_hex};

#[cfg(test)]
use parking_lot::Mutex;
#[cfg(test)]
use quotes::{next_quote_id, store_quote, take_quote_for};
#[cfg(test)]
use validate::estimated_fee_raw;

const LOG_PREFIX: &str = "[wallet]";

#[cfg(test)]
#[path = "execution_tests.rs"]
mod tests;
