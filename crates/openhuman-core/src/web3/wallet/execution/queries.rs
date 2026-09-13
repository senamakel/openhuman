//! Read-only wallet surface: network defaults, the supported-asset catalog,
//! per-chain provider status, and live balances.

use log::{debug, warn};

use crate::rpc::RpcOutcome;

use super::super::chains::{
    btc as chain_btc, evm as chain_evm, solana as chain_sol, tron as chain_tron,
};
use super::super::defaults::{
    evm_asset_catalog, network_defaults as default_networks, rpc_url_for_chain, EvmNetwork,
    WalletAssetDefinition, WalletNetworkDefaults,
};
use super::super::ops::{status as wallet_status, WalletChain, WALLET_NOT_CONFIGURED_MESSAGE};
use super::types::{BalanceInfo, ChainStatus, ProviderStatus, SupportedAsset};
use super::validate::{chain_str, format_amount};
use super::LOG_PREFIX;

pub(super) fn asset_to_supported(asset: WalletAssetDefinition) -> SupportedAsset {
    SupportedAsset {
        chain: asset.chain,
        evm_network: asset.evm_network,
        symbol: asset.symbol,
        name: asset.name,
        native: asset.native,
        decimals: asset.decimals,
        contract_address: asset.contract_address,
    }
}

pub async fn network_defaults() -> Result<RpcOutcome<Vec<WalletNetworkDefaults>>, String> {
    let rows = default_networks();
    debug!("{LOG_PREFIX} network_defaults count={}", rows.len());
    Ok(RpcOutcome::new(
        rows,
        vec!["wallet network defaults listed".to_string()],
    ))
}

pub async fn supported_assets() -> Result<RpcOutcome<Vec<SupportedAsset>>, String> {
    let mut assets: Vec<SupportedAsset> = Vec::new();
    for network in EvmNetwork::ALL {
        for asset in evm_asset_catalog(network) {
            assets.push(asset_to_supported(asset));
        }
    }
    for chain in [WalletChain::Btc, WalletChain::Solana, WalletChain::Tron] {
        for asset in super::super::defaults::asset_catalog(chain) {
            assets.push(asset_to_supported(asset));
        }
    }
    debug!("{LOG_PREFIX} supported_assets count={}", assets.len());
    Ok(RpcOutcome::new(
        assets,
        vec!["wallet supported_assets listed".to_string()],
    ))
}

pub async fn chain_status() -> Result<RpcOutcome<Vec<ChainStatus>>, String> {
    let status = wallet_status().await?.value;
    let mut rows = Vec::new();
    for network in EvmNetwork::ALL {
        let has_account = status
            .accounts
            .iter()
            .any(|account| account.chain == WalletChain::Evm);
        rows.push(ChainStatus {
            chain: WalletChain::Evm,
            evm_network: Some(network),
            configured: has_account,
            provider_status: if has_account {
                ProviderStatus::Ready
            } else {
                ProviderStatus::Missing
            },
            rpc_url: network.rpc_url(),
        });
    }
    for chain in [WalletChain::Btc, WalletChain::Solana, WalletChain::Tron] {
        let has_account = status.accounts.iter().any(|account| account.chain == chain);
        rows.push(ChainStatus {
            chain,
            evm_network: None,
            configured: has_account,
            provider_status: if has_account {
                ProviderStatus::Ready
            } else {
                ProviderStatus::Missing
            },
            rpc_url: rpc_url_for_chain(chain),
        });
    }
    debug!("{LOG_PREFIX} chain_status reported chains={}", rows.len());
    Ok(RpcOutcome::new(
        rows,
        vec!["wallet chain_status listed".to_string()],
    ))
}

/// EVM networks surfaced as their own native-balance rows. The single derived
/// EVM account address is shared across all of them, so `balances()` reads the
/// native asset (ETH / ETH / BNB) on each network independently.
pub const EVM_BALANCE_NETWORKS: [EvmNetwork; 3] = [
    EvmNetwork::EthereumMainnet,
    EvmNetwork::BaseMainnet,
    EvmNetwork::BscMainnet,
];

/// Build a single native-balance row, reading the live on-chain balance and
/// falling back to a zero/`Missing` row when the provider is unreachable.
fn balance_row(
    chain: WalletChain,
    evm_network: Option<EvmNetwork>,
    address: &str,
    asset: WalletAssetDefinition,
    raw: String,
    provider_status: ProviderStatus,
) -> BalanceInfo {
    let raw_u128 = raw.parse::<u128>().unwrap_or(0);
    BalanceInfo {
        chain,
        evm_network,
        address: address.to_string(),
        asset_symbol: asset.symbol,
        decimals: asset.decimals,
        formatted: format_amount(raw_u128, asset.decimals),
        raw,
        provider_status,
    }
}

fn native_asset_for(chain: WalletChain) -> Result<WalletAssetDefinition, String> {
    super::super::defaults::asset_catalog(chain)
        .into_iter()
        .find(|value| value.native)
        .ok_or_else(|| format!("native asset metadata missing for '{}'", chain_str(chain)))
}

fn evm_native_asset(network: EvmNetwork) -> Result<WalletAssetDefinition, String> {
    evm_asset_catalog(network)
        .into_iter()
        .find(|value| value.native)
        .ok_or_else(|| {
            format!(
                "native asset metadata missing for evm network '{}'",
                network.as_str()
            )
        })
}

pub async fn balances() -> Result<RpcOutcome<Vec<BalanceInfo>>, String> {
    let status = wallet_status().await?.value;
    if !status.configured {
        return Err(WALLET_NOT_CONFIGURED_MESSAGE.to_string());
    }
    let mut out = Vec::with_capacity(status.accounts.len() + EVM_BALANCE_NETWORKS.len());
    for account in &status.accounts {
        match account.chain {
            // The EVM account fans out into one native-balance row per displayed
            // network (Ethereum, Base, BNB Chain), all sharing the same address.
            WalletChain::Evm => {
                for network in EVM_BALANCE_NETWORKS {
                    let asset = evm_native_asset(network)?;
                    let (raw, provider_status) = match chain_evm::evm_balance(
                        network,
                        &account.address,
                    )
                    .await
                    {
                        Ok(balance) => (balance.to_string(), ProviderStatus::Ready),
                        Err(error) => {
                            warn!(
                                    "{LOG_PREFIX} balances chain=evm network={} address={} falling back to zero: {error}",
                                    network.as_str(),
                                    account.address
                                );
                            ("0".to_string(), ProviderStatus::Missing)
                        }
                    };
                    out.push(balance_row(
                        WalletChain::Evm,
                        Some(network),
                        &account.address,
                        asset,
                        raw,
                        provider_status,
                    ));
                }
            }
            WalletChain::Btc => {
                let (raw, provider_status) = match chain_btc::native_balance(&account.address).await
                {
                    Ok(sats) => (sats.to_string(), ProviderStatus::Ready),
                    Err(error) => {
                        warn!(
                            "{LOG_PREFIX} balances chain=btc address={} falling back to zero: {error}",
                            account.address
                        );
                        ("0".to_string(), ProviderStatus::Missing)
                    }
                };
                out.push(balance_row(
                    WalletChain::Btc,
                    None,
                    &account.address,
                    native_asset_for(WalletChain::Btc)?,
                    raw,
                    provider_status,
                ));
            }
            WalletChain::Solana => {
                let (raw, provider_status) = match chain_sol::native_balance(&account.address).await
                {
                    Ok(lamports) => (lamports.to_string(), ProviderStatus::Ready),
                    Err(error) => {
                        warn!(
                                "{LOG_PREFIX} balances chain=solana address={} falling back to zero: {error}",
                                account.address
                            );
                        ("0".to_string(), ProviderStatus::Missing)
                    }
                };
                out.push(balance_row(
                    WalletChain::Solana,
                    None,
                    &account.address,
                    native_asset_for(WalletChain::Solana)?,
                    raw,
                    provider_status,
                ));
            }
            WalletChain::Tron => {
                let (raw, provider_status) = match chain_tron::native_balance(&account.address)
                    .await
                {
                    Ok(sun) => (sun.to_string(), ProviderStatus::Ready),
                    Err(error) => {
                        warn!(
                            "{LOG_PREFIX} balances chain=tron address={} falling back to zero: {error}",
                            account.address
                        );
                        ("0".to_string(), ProviderStatus::Missing)
                    }
                };
                out.push(balance_row(
                    WalletChain::Tron,
                    None,
                    &account.address,
                    native_asset_for(WalletChain::Tron)?,
                    raw,
                    provider_status,
                ));
            }
        }
    }
    debug!("{LOG_PREFIX} balances returned rows={}", out.len());
    Ok(RpcOutcome::new(
        out,
        vec!["wallet balances listed".to_string()],
    ))
}
