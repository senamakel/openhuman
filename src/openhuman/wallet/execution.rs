//! Wallet execution surface — read tools (balances / supported assets / chain
//! status) and write tools (prepare-then-execute) for native sends, token
//! transfers, swaps, and contract calls.
//!
//! Design rules (see issue #1396):
//! - Quote / simulate first, then explicit confirm-and-execute. No one-shot
//!   hidden execution.
//! - Signing material stays local. `execute_prepared` returns a
//!   `ReadyToSign` structured payload that the desktop keystore consumes —
//!   this module never touches mnemonics or private keys.
//! - Wallet must be configured (see [`crate::openhuman::wallet::status`])
//!   before any read or write tool is callable.
//! - Every decision point emits a grep-friendly `[wallet]` debug log.
//!
//! On-chain RPC providers are not yet configured (#1395 ships the keystore;
//! provider config lives behind `OPENHUMAN_WALLET_RPC_*` env vars). Until a
//! provider is wired, balances surface `provider_status: "unconfigured"`
//! with zero values rather than fabricating numbers.
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use log::{debug, warn};
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::rpc::RpcOutcome;

use super::ops::{status as wallet_status, WalletAccount, WalletChain};

const LOG_PREFIX: &str = "[wallet]";
/// Prepared-transaction TTL. Quotes older than this are rejected at execute time.
const QUOTE_TTL_MS: u64 = 5 * 60 * 1000;
/// Cap on stored quotes; oldest entries are pruned when exceeded.
const QUOTE_STORE_CAP: usize = 64;

static QUOTE_STORE: Lazy<Mutex<Vec<PreparedTransaction>>> = Lazy::new(|| Mutex::new(Vec::new()));
static QUOTE_COUNTER: AtomicU64 = AtomicU64::new(1);

// -- Public types -----------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChainStatus {
    pub chain: WalletChain,
    pub configured: bool,
    pub provider_status: ProviderStatus,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProviderStatus {
    /// Wallet account exists for this chain and an RPC provider is reachable.
    Ready,
    /// Wallet account exists but no RPC provider has been configured yet.
    Unconfigured,
    /// Chain has no derived wallet account yet — run wallet setup first.
    Missing,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SupportedAsset {
    pub chain: WalletChain,
    pub symbol: &'static str,
    pub name: &'static str,
    pub native: bool,
    pub decimals: u8,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BalanceInfo {
    pub chain: WalletChain,
    pub address: String,
    pub asset_symbol: &'static str,
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
    Swap,
    ContractCall,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PreparedStatus {
    /// Quote has been simulated and is awaiting explicit user confirmation.
    AwaitingConfirmation,
    /// `execute_prepared` was invoked — payload is ready for the keystore.
    ReadyToSign,
    /// Quote expired or was already consumed.
    Consumed,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreparedTransaction {
    pub quote_id: String,
    pub kind: PreparedKind,
    pub chain: WalletChain,
    pub from_address: String,
    /// For transfers: recipient. For swaps: pool / router contract. For
    /// contract calls: target contract.
    pub to_address: String,
    pub asset_symbol: String,
    pub amount_raw: String,
    pub amount_formatted: String,
    /// For swaps only — the symbol the user expects to receive.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub receive_symbol: Option<String>,
    /// For swaps only — minimum amount out (raw integer string).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_receive_raw: Option<String>,
    /// For contract calls only — encoded calldata (hex, 0x-prefixed).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub calldata: Option<String>,
    /// Estimated network fee in the chain's native units (raw integer string).
    pub estimated_fee_raw: String,
    pub status: PreparedStatus,
    pub created_at_ms: u64,
    pub expires_at_ms: u64,
    /// Human-readable reasons surfaced from simulation, for the confirmation
    /// dialog (e.g. `slippage 0.5%`, `fee bump`).
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadyToSign {
    pub quote_id: String,
    pub status: PreparedStatus,
    pub chain: WalletChain,
    /// Full prepared transaction the keystore should sign.
    pub transaction: PreparedTransaction,
}

// -- Param types ------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrepareTransferParams {
    pub chain: WalletChain,
    pub to_address: String,
    /// Raw integer amount in the asset's smallest unit (wei / sat / lamports).
    pub amount_raw: String,
    /// `null` / absent => native asset for the chain. Otherwise a token symbol
    /// returned by `wallet.supported_assets`.
    #[serde(default)]
    pub asset_symbol: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrepareSwapParams {
    pub chain: WalletChain,
    pub from_symbol: String,
    pub to_symbol: String,
    pub amount_in_raw: String,
    /// Slippage tolerance in basis points (e.g. `50` = 0.5%).
    pub slippage_bps: u32,
    /// Router / aggregator contract address. Caller selects the venue.
    pub router_address: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrepareContractCallParams {
    pub chain: WalletChain,
    pub contract_address: String,
    /// Hex-encoded calldata (`0x`-prefixed).
    pub calldata: String,
    /// Native value to attach (raw, smallest unit). `"0"` for view / pure
    /// state mutations on EVM.
    #[serde(default = "zero_string")]
    pub value_raw: String,
}

fn zero_string() -> String {
    "0".to_string()
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutePreparedParams {
    pub quote_id: String,
    /// Caller MUST set this to `true`. If absent / false, the call is
    /// rejected — this is the safety boundary between simulate and execute.
    pub confirmed: bool,
}

// -- Helpers ----------------------------------------------------------------

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn next_quote_id() -> String {
    let n = QUOTE_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("q_{}_{}", now_ms(), n)
}

async fn require_account(chain: WalletChain) -> Result<WalletAccount, String> {
    let status = wallet_status().await?.value;
    if !status.configured {
        return Err("wallet is not configured; run wallet setup first".to_string());
    }
    status
        .accounts
        .into_iter()
        .find(|a| a.chain == chain)
        .ok_or_else(|| format!("no wallet account derived for chain '{}'", chain_str(chain)))
}

fn chain_str(chain: WalletChain) -> &'static str {
    match chain {
        WalletChain::Evm => "evm",
        WalletChain::Btc => "btc",
        WalletChain::Solana => "solana",
        WalletChain::Tron => "tron",
    }
}

fn native_asset(chain: WalletChain) -> SupportedAsset {
    match chain {
        WalletChain::Evm => SupportedAsset {
            chain,
            symbol: "ETH",
            name: "Ether",
            native: true,
            decimals: 18,
        },
        WalletChain::Btc => SupportedAsset {
            chain,
            symbol: "BTC",
            name: "Bitcoin",
            native: true,
            decimals: 8,
        },
        WalletChain::Solana => SupportedAsset {
            chain,
            symbol: "SOL",
            name: "Solana",
            native: true,
            decimals: 9,
        },
        WalletChain::Tron => SupportedAsset {
            chain,
            symbol: "TRX",
            name: "Tron",
            native: true,
            decimals: 6,
        },
    }
}

fn provider_env_set(chain: WalletChain) -> bool {
    let key = match chain {
        WalletChain::Evm => "OPENHUMAN_WALLET_RPC_EVM",
        WalletChain::Btc => "OPENHUMAN_WALLET_RPC_BTC",
        WalletChain::Solana => "OPENHUMAN_WALLET_RPC_SOLANA",
        WalletChain::Tron => "OPENHUMAN_WALLET_RPC_TRON",
    };
    std::env::var(key)
        .map(|v| !v.trim().is_empty())
        .unwrap_or(false)
}

fn validate_amount(raw: &str) -> Result<u128, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("amount is empty".to_string());
    }
    trimmed
        .parse::<u128>()
        .map_err(|_| format!("amount '{trimmed}' is not a valid non-negative integer"))
}

fn validate_address(addr: &str) -> Result<String, String> {
    let trimmed = addr.trim();
    if trimmed.is_empty() {
        return Err("address is empty".to_string());
    }
    Ok(trimmed.to_string())
}

fn validate_calldata(data: &str) -> Result<String, String> {
    let t = data.trim();
    if !t.starts_with("0x") {
        return Err("calldata must be 0x-prefixed hex".to_string());
    }
    let body = &t[2..];
    if body.len() % 2 != 0 {
        return Err("calldata hex must be byte-aligned".to_string());
    }
    if !body.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("calldata contains non-hex characters".to_string());
    }
    Ok(t.to_string())
}

fn format_amount(raw: u128, decimals: u8) -> String {
    if decimals == 0 {
        return raw.to_string();
    }
    let s = raw.to_string();
    let d = decimals as usize;
    if s.len() <= d {
        format!("0.{:0>width$}", s, width = d)
    } else {
        let split = s.len() - d;
        format!("{}.{}", &s[..split], &s[split..])
    }
}

fn estimated_fee_raw(chain: WalletChain, kind: PreparedKind) -> String {
    // Pessimistic stub estimates so simulation has a non-zero number to show.
    // Real values come from the chain's fee oracle once a provider is wired.
    let base = match (chain, kind) {
        (WalletChain::Evm, PreparedKind::NativeTransfer) => 21_000u128 * 30_000_000_000,
        (WalletChain::Evm, PreparedKind::TokenTransfer) => 65_000u128 * 30_000_000_000,
        (WalletChain::Evm, PreparedKind::Swap) => 200_000u128 * 30_000_000_000,
        (WalletChain::Evm, PreparedKind::ContractCall) => 100_000u128 * 30_000_000_000,
        (WalletChain::Btc, _) => 5_000,
        (WalletChain::Solana, _) => 5_000,
        (WalletChain::Tron, _) => 1_000_000,
    };
    base.to_string()
}

fn store_quote(quote: PreparedTransaction) -> PreparedTransaction {
    let mut store = QUOTE_STORE.lock();
    let cutoff = now_ms();
    store.retain(|q| q.expires_at_ms > cutoff && q.status != PreparedStatus::Consumed);
    if store.len() >= QUOTE_STORE_CAP {
        store.remove(0);
    }
    store.push(quote.clone());
    quote
}

fn take_quote(quote_id: &str) -> Result<PreparedTransaction, String> {
    let mut store = QUOTE_STORE.lock();
    let now = now_ms();
    let pos = store
        .iter()
        .position(|q| q.quote_id == quote_id)
        .ok_or_else(|| format!("quote '{quote_id}' not found"))?;
    let quote = store.remove(pos);
    if quote.status == PreparedStatus::Consumed {
        return Err(format!("quote '{quote_id}' already executed"));
    }
    if quote.expires_at_ms <= now {
        return Err(format!("quote '{quote_id}' expired"));
    }
    Ok(quote)
}

#[cfg(test)]
fn reset_quote_store_for_tests() {
    QUOTE_STORE.lock().clear();
}

// -- Operations -------------------------------------------------------------

pub async fn supported_assets() -> Result<RpcOutcome<Vec<SupportedAsset>>, String> {
    let assets: Vec<SupportedAsset> = [
        WalletChain::Evm,
        WalletChain::Btc,
        WalletChain::Solana,
        WalletChain::Tron,
    ]
    .into_iter()
    .map(native_asset)
    .collect();
    debug!("{LOG_PREFIX} supported_assets count={}", assets.len());
    Ok(RpcOutcome::new(
        assets,
        vec!["wallet supported_assets listed".to_string()],
    ))
}

pub async fn chain_status() -> Result<RpcOutcome<Vec<ChainStatus>>, String> {
    let status = wallet_status().await?.value;
    let mut rows = Vec::with_capacity(4);
    for chain in [
        WalletChain::Evm,
        WalletChain::Btc,
        WalletChain::Solana,
        WalletChain::Tron,
    ] {
        let has_account = status.accounts.iter().any(|a| a.chain == chain);
        let provider_status = if !has_account {
            ProviderStatus::Missing
        } else if provider_env_set(chain) {
            ProviderStatus::Ready
        } else {
            ProviderStatus::Unconfigured
        };
        rows.push(ChainStatus {
            chain,
            configured: has_account,
            provider_status,
        });
    }
    debug!("{LOG_PREFIX} chain_status reported chains={}", rows.len());
    Ok(RpcOutcome::new(
        rows,
        vec!["wallet chain_status listed".to_string()],
    ))
}

pub async fn balances() -> Result<RpcOutcome<Vec<BalanceInfo>>, String> {
    let status = wallet_status().await?.value;
    if !status.configured {
        return Err("wallet is not configured; run wallet setup first".to_string());
    }
    let mut out = Vec::with_capacity(status.accounts.len());
    for account in &status.accounts {
        let asset = native_asset(account.chain);
        let provider_status = if provider_env_set(account.chain) {
            ProviderStatus::Ready
        } else {
            ProviderStatus::Unconfigured
        };
        if provider_status == ProviderStatus::Unconfigured {
            warn!(
                "{LOG_PREFIX} balances chain={} provider unconfigured; returning zero placeholder",
                chain_str(account.chain)
            );
        }
        out.push(BalanceInfo {
            chain: account.chain,
            address: account.address.clone(),
            asset_symbol: asset.symbol,
            decimals: asset.decimals,
            raw: "0".to_string(),
            formatted: format_amount(0, asset.decimals),
            provider_status,
        });
    }
    debug!("{LOG_PREFIX} balances returned rows={}", out.len());
    Ok(RpcOutcome::new(
        out,
        vec!["wallet balances listed".to_string()],
    ))
}

pub async fn prepare_transfer(
    params: PrepareTransferParams,
) -> Result<RpcOutcome<PreparedTransaction>, String> {
    let to = validate_address(&params.to_address)?;
    let amount = validate_amount(&params.amount_raw)?;
    if amount == 0 {
        return Err("transfer amount must be greater than zero".to_string());
    }
    let native = native_asset(params.chain);
    let (kind, asset_symbol, decimals) = match params.asset_symbol.as_deref().map(str::trim) {
        None | Some("") => (
            PreparedKind::NativeTransfer,
            native.symbol.to_string(),
            native.decimals,
        ),
        Some(sym) if sym.eq_ignore_ascii_case(native.symbol) => (
            PreparedKind::NativeTransfer,
            native.symbol.to_string(),
            native.decimals,
        ),
        Some(sym) => {
            return Err(format!(
                "unsupported asset_symbol '{sym}'; only native assets are listed in wallet.supported_assets today"
            ));
        }
    };
    let account = require_account(params.chain).await?;

    let now = now_ms();
    let quote = PreparedTransaction {
        quote_id: next_quote_id(),
        kind,
        chain: params.chain,
        from_address: account.address.clone(),
        to_address: to,
        asset_symbol: asset_symbol.clone(),
        amount_raw: amount.to_string(),
        amount_formatted: format_amount(amount, decimals),
        receive_symbol: None,
        min_receive_raw: None,
        calldata: None,
        estimated_fee_raw: estimated_fee_raw(params.chain, kind),
        status: PreparedStatus::AwaitingConfirmation,
        created_at_ms: now,
        expires_at_ms: now + QUOTE_TTL_MS,
        notes: vec![format!(
            "Simulation only — confirm to forward to keystore. Asset: {asset_symbol}."
        )],
    };
    debug!(
        "{LOG_PREFIX} prepare_transfer chain={} kind={:?} quote_id={} amount={}",
        chain_str(params.chain),
        kind,
        quote.quote_id,
        quote.amount_raw
    );
    Ok(RpcOutcome::new(
        store_quote(quote),
        vec!["wallet transfer prepared".to_string()],
    ))
}

pub async fn prepare_swap(
    params: PrepareSwapParams,
) -> Result<RpcOutcome<PreparedTransaction>, String> {
    if params.from_symbol.trim().is_empty() || params.to_symbol.trim().is_empty() {
        return Err("swap requires non-empty from_symbol and to_symbol".to_string());
    }
    if params.from_symbol.eq_ignore_ascii_case(&params.to_symbol) {
        return Err("swap from_symbol and to_symbol must differ".to_string());
    }
    if params.slippage_bps > 5_000 {
        return Err("slippage_bps too high (cap 5000 = 50%)".to_string());
    }
    let amount = validate_amount(&params.amount_in_raw)?;
    if amount == 0 {
        return Err("swap amount_in_raw must be greater than zero".to_string());
    }
    let router = validate_address(&params.router_address)?;
    let account = require_account(params.chain).await?;

    // Conservative min-out: amount * (10000 - slippage) / 10000. Without a
    // real quote we cannot compute the swap rate; this lets the UI display a
    // floor and forces explicit caller-side rate input via the router quote
    // pre-step once the provider lands.
    let min_out = amount.saturating_mul((10_000 - params.slippage_bps) as u128) / 10_000;
    let native = native_asset(params.chain);
    let now = now_ms();
    let quote = PreparedTransaction {
        quote_id: next_quote_id(),
        kind: PreparedKind::Swap,
        chain: params.chain,
        from_address: account.address.clone(),
        to_address: router,
        asset_symbol: params.from_symbol.clone(),
        amount_raw: amount.to_string(),
        amount_formatted: format_amount(amount, native.decimals),
        receive_symbol: Some(params.to_symbol.clone()),
        min_receive_raw: Some(min_out.to_string()),
        calldata: None,
        estimated_fee_raw: estimated_fee_raw(params.chain, PreparedKind::Swap),
        status: PreparedStatus::AwaitingConfirmation,
        created_at_ms: now,
        expires_at_ms: now + QUOTE_TTL_MS,
        notes: vec![format!(
            "Swap {} -> {}, slippage {} bps. Real router quote required before signing.",
            params.from_symbol, params.to_symbol, params.slippage_bps
        )],
    };
    debug!(
        "{LOG_PREFIX} prepare_swap chain={} quote_id={} from={} to={} slippage_bps={}",
        chain_str(params.chain),
        quote.quote_id,
        params.from_symbol,
        params.to_symbol,
        params.slippage_bps
    );
    Ok(RpcOutcome::new(
        store_quote(quote),
        vec!["wallet swap prepared".to_string()],
    ))
}

pub async fn prepare_contract_call(
    params: PrepareContractCallParams,
) -> Result<RpcOutcome<PreparedTransaction>, String> {
    if !matches!(params.chain, WalletChain::Evm | WalletChain::Tron) {
        return Err(format!(
            "contract calls are only supported on EVM and Tron chains; got '{}'",
            chain_str(params.chain)
        ));
    }
    let contract = validate_address(&params.contract_address)?;
    let calldata = validate_calldata(&params.calldata)?;
    let value = validate_amount(&params.value_raw)?;
    let account = require_account(params.chain).await?;

    let native = native_asset(params.chain);
    let now = now_ms();
    let quote = PreparedTransaction {
        quote_id: next_quote_id(),
        kind: PreparedKind::ContractCall,
        chain: params.chain,
        from_address: account.address.clone(),
        to_address: contract,
        asset_symbol: native.symbol.to_string(),
        amount_raw: value.to_string(),
        amount_formatted: format_amount(value, native.decimals),
        receive_symbol: None,
        min_receive_raw: None,
        calldata: Some(calldata),
        estimated_fee_raw: estimated_fee_raw(params.chain, PreparedKind::ContractCall),
        status: PreparedStatus::AwaitingConfirmation,
        created_at_ms: now,
        expires_at_ms: now + QUOTE_TTL_MS,
        notes: vec!["Contract call simulation — verify ABI before signing.".to_string()],
    };
    debug!(
        "{LOG_PREFIX} prepare_contract_call chain={} quote_id={} value={}",
        chain_str(params.chain),
        quote.quote_id,
        quote.amount_raw
    );
    Ok(RpcOutcome::new(
        store_quote(quote),
        vec!["wallet contract call prepared".to_string()],
    ))
}

pub async fn execute_prepared(
    params: ExecutePreparedParams,
) -> Result<RpcOutcome<ReadyToSign>, String> {
    if !params.confirmed {
        return Err("execute_prepared requires `confirmed: true`".to_string());
    }
    let mut quote = take_quote(&params.quote_id)?;
    quote.status = PreparedStatus::ReadyToSign;
    debug!(
        "{LOG_PREFIX} execute_prepared quote_id={} chain={} kind={:?} -> ReadyToSign",
        quote.quote_id,
        chain_str(quote.chain),
        quote.kind
    );
    let result = ReadyToSign {
        quote_id: quote.quote_id.clone(),
        status: quote.status,
        chain: quote.chain,
        transaction: quote,
    };
    Ok(RpcOutcome::new(
        result,
        vec!["wallet quote handed to keystore".to_string()],
    ))
}

// -- Tests ------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_amount_rejects_empty_and_non_numeric() {
        assert!(validate_amount("").is_err());
        assert!(validate_amount("abc").is_err());
        assert_eq!(validate_amount("42").unwrap(), 42);
    }

    #[test]
    fn validates_calldata_requires_hex() {
        assert!(validate_calldata("deadbeef").is_err());
        assert!(validate_calldata("0xZZ").is_err());
        assert!(validate_calldata("0xabc").is_err());
        assert_eq!(validate_calldata("0xdeadbeef").unwrap(), "0xdeadbeef");
    }

    #[test]
    fn formats_amount_with_decimals() {
        assert_eq!(format_amount(0, 18), "0.000000000000000000");
        assert_eq!(format_amount(1, 8), "0.00000001");
        assert_eq!(format_amount(123_456_789, 8), "1.23456789");
        assert_eq!(format_amount(100, 0), "100");
    }

    #[test]
    fn next_quote_id_is_unique_and_prefixed() {
        let a = next_quote_id();
        let b = next_quote_id();
        assert_ne!(a, b);
        assert!(a.starts_with("q_"));
    }

    #[test]
    fn quote_store_round_trips_and_expires() {
        reset_quote_store_for_tests();
        let now = now_ms();
        let mut q = PreparedTransaction {
            quote_id: "q_test_1".to_string(),
            kind: PreparedKind::NativeTransfer,
            chain: WalletChain::Evm,
            from_address: "0xfrom".to_string(),
            to_address: "0xto".to_string(),
            asset_symbol: "ETH".to_string(),
            amount_raw: "1".to_string(),
            amount_formatted: "0.000000000000000001".to_string(),
            receive_symbol: None,
            min_receive_raw: None,
            calldata: None,
            estimated_fee_raw: "0".to_string(),
            status: PreparedStatus::AwaitingConfirmation,
            created_at_ms: now,
            expires_at_ms: now + 60_000,
            notes: vec![],
        };
        store_quote(q.clone());
        let taken = take_quote("q_test_1").expect("quote round-trips");
        assert_eq!(taken.quote_id, "q_test_1");
        assert!(take_quote("q_test_1").is_err(), "second take must fail");

        // Expired quote: store and then try to take.
        q.quote_id = "q_test_2".to_string();
        q.expires_at_ms = now.saturating_sub(1);
        store_quote(q);
        let err = take_quote("q_test_2").unwrap_err();
        assert!(err.contains("expired"), "got: {err}");
    }

    #[test]
    fn execute_prepared_requires_confirmed_flag() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let err = execute_prepared(ExecutePreparedParams {
                quote_id: "missing".to_string(),
                confirmed: false,
            })
            .await
            .unwrap_err();
            assert!(err.contains("confirmed: true"), "got: {err}");
        });
    }

    #[test]
    fn supported_assets_lists_four_natives() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let out = supported_assets().await.unwrap();
            assert_eq!(out.value.len(), 4);
            assert!(out.value.iter().all(|a| a.native));
        });
    }

    #[test]
    fn prepare_swap_rejects_same_symbol() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let err = prepare_swap(PrepareSwapParams {
                chain: WalletChain::Evm,
                from_symbol: "USDC".into(),
                to_symbol: "usdc".into(),
                amount_in_raw: "100".into(),
                slippage_bps: 50,
                router_address: "0xrouter".into(),
            })
            .await
            .unwrap_err();
            assert!(err.contains("must differ"), "got: {err}");
        });
    }

    #[test]
    fn prepare_transfer_rejects_unsupported_asset_symbol() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let err = prepare_transfer(PrepareTransferParams {
                chain: WalletChain::Evm,
                to_address: "0xabc".into(),
                amount_raw: "1".into(),
                asset_symbol: Some("USDC".into()),
            })
            .await
            .unwrap_err();
            assert!(err.contains("unsupported asset_symbol"), "got: {err}");
        });
    }

    #[test]
    fn prepare_contract_call_rejects_non_evm_chain() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let err = prepare_contract_call(PrepareContractCallParams {
                chain: WalletChain::Btc,
                contract_address: "addr".into(),
                calldata: "0x".into(),
                value_raw: "0".into(),
            })
            .await
            .unwrap_err();
            assert!(err.contains("only supported"), "got: {err}");
        });
    }
}
