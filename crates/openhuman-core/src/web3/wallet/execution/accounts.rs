//! Resolving a derived wallet account for a chain, erroring cleanly when the
//! wallet has not been configured yet.

use super::super::ops::{
    status as wallet_status, WalletAccount, WalletChain, WALLET_NOT_CONFIGURED_MESSAGE,
};
use super::validate::chain_str;

/// Resolve the derived EVM account address, erroring if the wallet is not
/// configured. Used by the `web3` signing primitives that operate on the
/// single shared EVM address.
pub(crate) async fn require_evm_account() -> Result<String, String> {
    Ok(require_account(WalletChain::Evm).await?.address)
}

pub(super) async fn require_account(chain: WalletChain) -> Result<WalletAccount, String> {
    let status = wallet_status().await?.value;
    if !status.configured {
        return Err(WALLET_NOT_CONFIGURED_MESSAGE.to_string());
    }
    status
        .accounts
        .into_iter()
        .find(|account| account.chain == chain)
        .ok_or_else(|| format!("no wallet account derived for chain '{}'", chain_str(chain)))
}
