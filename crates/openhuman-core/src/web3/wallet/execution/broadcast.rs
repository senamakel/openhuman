//! Signing and broadcasting: the crate-internal raw primitives the `web3`
//! layer uses for externally-built transactions, and `execute_prepared`,
//! which atomically consumes a quote and dispatches it to the right chain.

use log::warn;

use crate::rpc::RpcOutcome;

use super::super::chains::{
    btc as chain_btc, evm as chain_evm, solana as chain_sol, tron as chain_tron,
};
use super::super::defaults::{explorer_tx_url, EvmNetwork};
use super::super::ops::WalletChain;
use super::quotes::{current_owner, now_ms, store_quote, take_quote_for, QUOTE_TTL_MS};
use super::types::{ExecutePreparedParams, ExecutionResult, RawBroadcastResult};
use super::validate::chain_str;
use super::LOG_PREFIX;

/// Crate-internal: sign+broadcast an externally-built unsigned EVM transaction
/// (deBridge swap/bridge or generic dapp calldata). See [`chain_evm::sign_and_broadcast_evm`].
pub(crate) async fn sign_and_broadcast_evm(
    network: EvmNetwork,
    to: &str,
    data_hex: Option<String>,
    value_raw: &str,
) -> Result<RawBroadcastResult, String> {
    chain_evm::sign_and_broadcast_evm(network, to, data_hex, value_raw).await
}

/// Crate-internal: sign+broadcast an externally-built hex `VersionedTransaction`
/// (deBridge Solana swap/bridge). See [`chain_sol::sign_and_broadcast_versioned`].
pub(crate) async fn sign_and_broadcast_solana(
    tx_blob_hex: &str,
) -> Result<RawBroadcastResult, String> {
    chain_sol::sign_and_broadcast_versioned(tx_blob_hex).await
}

pub async fn execute_prepared(
    params: ExecutePreparedParams,
) -> Result<RpcOutcome<ExecutionResult>, String> {
    if !params.confirmed {
        return Err("execute_prepared requires `confirmed: true`".to_string());
    }
    // Bind execute to the chat-thread that prepared the quote.
    // `current_owner()` returns the caller's `APPROVAL_CHAT_CONTEXT` (or `None`
    // for non-chat callers). `take_quote_for` enforces equality with the
    // prepare-time owner and returns the same "not found" error on mismatch
    // — leaked `quote_id`s in a shared channel cannot be hijacked from a
    // different agent session.
    let caller = current_owner();
    // Atomically remove the quote *before* broadcasting so two concurrent
    // confirmations can't both pass get_quote() and double-submit. If signing
    // or broadcast fails we restore the quote to keep it retryable.
    let quote = take_quote_for(&params.quote_id, caller)?;
    let chain = quote.chain;
    let restorable = quote.clone();
    let result = match chain {
        WalletChain::Evm => chain_evm::execute_evm_quote(quote).await,
        WalletChain::Btc => chain_btc::execute_btc_quote(quote).await,
        WalletChain::Solana => chain_sol::execute_solana_quote(quote).await,
        WalletChain::Tron => chain_tron::execute_tron_quote(quote).await,
    };
    let result = match result {
        Ok(value) => value,
        Err(error) => {
            // Restore the quote so the caller can fix the cause and retry.
            // Refresh the TTL window so a slow chain call (network timeouts
            // can chew through the original 5-min budget) doesn't hand back
            // an immediately-expired quote.
            let mut refreshed = restorable;
            let now = now_ms();
            refreshed.expires_at_ms = now + QUOTE_TTL_MS;
            store_quote(refreshed);
            warn!(
                "{LOG_PREFIX} execute chain={} quote_id={} failed (quote restored, ttl refreshed): {error}",
                chain_str(chain),
                params.quote_id
            );
            return Err(error);
        }
    };
    let explorer_fallback = explorer_tx_url(chain, &result.transaction_hash);
    let mut final_result = result;
    if final_result.explorer_url.is_none() {
        final_result.explorer_url = explorer_fallback;
    }
    Ok(RpcOutcome::new(
        final_result,
        vec!["wallet transaction broadcast".to_string()],
    ))
}
