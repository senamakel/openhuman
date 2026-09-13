//! Preparing a native/token transfer quote: resolve the network, the asset,
//! and the source account, then stamp a `PreparedTransaction` the caller
//! must separately confirm through `execute_prepared`.

use log::debug;

use crate::rpc::RpcOutcome;

use super::super::defaults::{evm_asset_catalog, find_asset_for_network, EvmNetwork};
use super::super::ops::WalletChain;
use super::accounts::require_account;
use super::quotes::{current_owner, next_quote_id, now_ms, store_quote, QUOTE_TTL_MS};
use super::types::{PrepareTransferParams, PreparedKind, PreparedStatus, PreparedTransaction};
use super::validate::{
    chain_str, estimated_fee_raw, format_amount, validate_address, validate_amount,
};
use super::LOG_PREFIX;

pub async fn prepare_transfer(
    params: PrepareTransferParams,
) -> Result<RpcOutcome<PreparedTransaction>, String> {
    let to = validate_address(params.chain, &params.to_address)?;
    let amount = validate_amount(&params.amount_raw)?;
    if amount == 0 {
        return Err("transfer amount must be greater than zero".to_string());
    }
    let network = if params.chain == WalletChain::Evm {
        Some(params.evm_network.unwrap_or(EvmNetwork::EthereumMainnet))
    } else {
        None
    };
    let account = require_account(params.chain).await?;
    let asset = match params.asset_symbol.as_deref().map(str::trim) {
        None | Some("") => {
            // native asset for the chain (or chosen EVM network).
            let catalog = if let Some(net) = network {
                evm_asset_catalog(net)
            } else {
                super::super::defaults::asset_catalog(params.chain)
            };
            catalog
                .into_iter()
                .find(|value| value.native)
                .ok_or_else(|| {
                    format!(
                        "native asset metadata missing for '{}'",
                        chain_str(params.chain)
                    )
                })?
        }
        Some(symbol) => find_asset_for_network(params.chain, network, symbol).ok_or_else(|| {
            format!(
                "unsupported asset_symbol '{symbol}' for chain '{}'",
                chain_str(params.chain)
            )
        })?,
    };
    let kind = if asset.native {
        PreparedKind::NativeTransfer
    } else {
        PreparedKind::TokenTransfer
    };
    // BTC has no native token concept; reject TokenTransfer on btc.
    if matches!(params.chain, WalletChain::Btc) && !asset.native {
        return Err("token transfers are not supported on Bitcoin".to_string());
    }
    let now = now_ms();
    let label = if let Some(net) = network {
        format!("{} ({})", chain_str(params.chain), net.network_label())
    } else {
        chain_str(params.chain).to_string()
    };
    let quote = PreparedTransaction {
        quote_id: next_quote_id(),
        kind,
        chain: params.chain,
        evm_network: network,
        from_address: account.address.clone(),
        to_address: to,
        asset_symbol: asset.symbol.clone(),
        amount_raw: amount.to_string(),
        amount_formatted: format_amount(amount, asset.decimals),
        receive_symbol: None,
        min_receive_raw: None,
        calldata: None,
        token_address: asset.contract_address.clone(),
        estimated_fee_raw: estimated_fee_raw(params.chain, kind),
        status: PreparedStatus::AwaitingConfirmation,
        created_at_ms: now,
        expires_at_ms: now + QUOTE_TTL_MS,
        notes: vec![format!(
            "Prepared {} transfer on {} using default network settings.",
            asset.symbol, label
        )],
        owner: current_owner(),
    };
    debug!(
        "{LOG_PREFIX} prepare_transfer chain={} kind={:?} quote_id={} amount={} asset={}",
        chain_str(params.chain),
        kind,
        quote.quote_id,
        quote.amount_raw,
        quote.asset_symbol
    );
    Ok(RpcOutcome::new(
        store_quote(quote),
        vec!["wallet transfer prepared".to_string()],
    ))
}
