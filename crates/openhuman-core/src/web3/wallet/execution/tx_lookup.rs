//! Read-only transaction lookups: lifecycle state, receipt, and raw payload,
//! dispatched to the chain-specific client.

use log::debug;

use crate::rpc::RpcOutcome;

use super::super::chains::{
    btc as chain_btc, evm as chain_evm, solana as chain_sol, tron as chain_tron,
};
use super::super::defaults::EvmNetwork;
use super::super::ops::WalletChain;
use super::types::{TxLookupInfo, TxReceiptInfo, TxStatusInfo};
use super::validate::chain_str;
use super::LOG_PREFIX;

/// Resolve the EVM network for a tx read, defaulting to Ethereum mainnet.
fn read_network(chain: WalletChain, evm_network: Option<EvmNetwork>) -> Option<EvmNetwork> {
    if chain == WalletChain::Evm {
        Some(evm_network.unwrap_or(EvmNetwork::EthereumMainnet))
    } else {
        None
    }
}

/// Check the on-chain lifecycle state of a previously broadcast transaction.
pub async fn tx_status(
    chain: WalletChain,
    evm_network: Option<EvmNetwork>,
    hash: &str,
) -> Result<RpcOutcome<TxStatusInfo>, String> {
    let hash = hash.trim();
    if hash.is_empty() {
        return Err("tx hash is empty".to_string());
    }
    let info = match chain {
        WalletChain::Evm => {
            chain_evm::tx_status(read_network(chain, evm_network).unwrap(), hash).await?
        }
        WalletChain::Btc => chain_btc::tx_status(hash).await?,
        WalletChain::Solana => chain_sol::tx_status(hash).await?,
        WalletChain::Tron => chain_tron::tx_status(hash).await?,
    };
    debug!(
        "{LOG_PREFIX} tx_status chain={} hash={} state={:?}",
        chain_str(chain),
        hash,
        info.state
    );
    Ok(RpcOutcome::new(
        info,
        vec!["wallet tx status fetched".to_string()],
    ))
}

/// Fetch the receipt of a broadcast transaction (success flag, fee, block).
pub async fn tx_receipt(
    chain: WalletChain,
    evm_network: Option<EvmNetwork>,
    hash: &str,
) -> Result<RpcOutcome<TxReceiptInfo>, String> {
    let hash = hash.trim();
    if hash.is_empty() {
        return Err("tx hash is empty".to_string());
    }
    let info = match chain {
        WalletChain::Evm => {
            chain_evm::tx_receipt(read_network(chain, evm_network).unwrap(), hash).await?
        }
        WalletChain::Btc => chain_btc::tx_receipt(hash).await?,
        WalletChain::Solana => chain_sol::tx_receipt(hash).await?,
        WalletChain::Tron => chain_tron::tx_receipt(hash).await?,
    };
    debug!(
        "{LOG_PREFIX} tx_receipt chain={} hash={} found={}",
        chain_str(chain),
        hash,
        info.found
    );
    Ok(RpcOutcome::new(
        info,
        vec!["wallet tx receipt fetched".to_string()],
    ))
}

/// Look up the raw transaction payload by hash.
pub async fn lookup_tx(
    chain: WalletChain,
    evm_network: Option<EvmNetwork>,
    hash: &str,
) -> Result<RpcOutcome<TxLookupInfo>, String> {
    let hash = hash.trim();
    if hash.is_empty() {
        return Err("tx hash is empty".to_string());
    }
    let info = match chain {
        WalletChain::Evm => {
            chain_evm::lookup_tx(read_network(chain, evm_network).unwrap(), hash).await?
        }
        WalletChain::Btc => chain_btc::lookup_tx(hash).await?,
        WalletChain::Solana => chain_sol::lookup_tx(hash).await?,
        WalletChain::Tron => chain_tron::lookup_tx(hash).await?,
    };
    debug!(
        "{LOG_PREFIX} lookup_tx chain={} hash={} found={}",
        chain_str(chain),
        hash,
        info.found
    );
    Ok(RpcOutcome::new(
        info,
        vec!["wallet tx looked up".to_string()],
    ))
}
