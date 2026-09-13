//! Address/amount/calldata validation, amount formatting, hex conversions,
//! and the estimated-fee table used when preparing a quote.

use log::debug;

use super::super::ops::WalletChain;
use super::types::PreparedKind;
use super::LOG_PREFIX;

/// Return the compressed SEC1 public key for a secp256k1 secret.
///
/// Test-only. Production never holds a secp256k1 secret: the wallet module
/// derives the key and reports the public half through `DeriveAccount`.
#[cfg(test)]
pub(crate) fn compressed_public_key(secret: &[u8]) -> Result<Vec<u8>, String> {
    let key = k256::ecdsa::SigningKey::from_slice(secret)
        .map_err(|_| "derived key is not a valid secp256k1 scalar".to_string())?;
    Ok(key
        .verifying_key()
        .to_encoded_point(true)
        .as_bytes()
        .to_vec())
}

pub(crate) fn chain_str(chain: WalletChain) -> &'static str {
    match chain {
        WalletChain::Evm => "evm",
        WalletChain::Btc => "btc",
        WalletChain::Solana => "solana",
        WalletChain::Tron => "tron",
    }
}

pub(crate) fn validate_amount(raw: &str) -> Result<u128, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("amount is empty".to_string());
    }
    trimmed
        .parse::<u128>()
        .map_err(|_| format!("amount '{trimmed}' is not a valid non-negative integer"))
}

/// Validate `addr` for `chain`, returning it trimmed.
///
/// Every arm delegates to the vendored [`tinywallet_bus`] crate, which owns the
/// four address formats. The dispatch stays here rather than calling
/// `tinywallet_bus::address::validate` directly because [`WalletChain`] is
/// OpenHuman's enum, and mapping it onto `tinywallet_bus::Chain` here keeps that
/// translation in one place.
///
/// For Bitcoin this is the **recipient** rule — any well-formed mainnet
/// address. Sender addresses go through `chain_btc::validate_btc_sender_address`,
/// which additionally requires P2WPKH; the distinction has no equivalent on
/// the other three chains, so it cannot be expressed through this entry point.
pub(super) fn validate_address(chain: WalletChain, addr: &str) -> Result<String, String> {
    let tw_chain = match chain {
        WalletChain::Evm => tinywallet_bus::Chain::Evm,
        WalletChain::Btc => tinywallet_bus::Chain::Btc,
        WalletChain::Solana => tinywallet_bus::Chain::Solana,
        WalletChain::Tron => tinywallet_bus::Chain::Tron,
    };
    debug!("{LOG_PREFIX} validate_address chain={chain:?} role=recipient dispatch=tinywallet_bus");
    let result = tinywallet_bus::address::validate(tw_chain, addr).map_err(|e| e.to_string());
    debug!(
        "{LOG_PREFIX} validate_address chain={chain:?} role=recipient result={}",
        if result.is_ok() {
            "accepted"
        } else {
            "rejected"
        }
    );
    result
}

pub(crate) fn validate_calldata(data: &str) -> Result<String, String> {
    let trimmed = data.trim();
    if !trimmed.starts_with("0x") {
        return Err("calldata must be 0x-prefixed hex".to_string());
    }
    let body = &trimmed[2..];
    if !body.len().is_multiple_of(2) {
        return Err("calldata hex must be byte-aligned".to_string());
    }
    if !body.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("calldata contains non-hex characters".to_string());
    }
    Ok(trimmed.to_string())
}

pub(crate) fn format_amount(raw: u128, decimals: u8) -> String {
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

pub(super) fn estimated_fee_raw(chain: WalletChain, kind: PreparedKind) -> String {
    let base = match (chain, kind) {
        (WalletChain::Evm, PreparedKind::NativeTransfer) => 21_000u128 * 30_000_000_000,
        (WalletChain::Evm, PreparedKind::TokenTransfer) => 65_000u128 * 30_000_000_000,
        (WalletChain::Btc, _) => 5_000,
        (WalletChain::Solana, _) => 5_000,
        (WalletChain::Tron, PreparedKind::NativeTransfer) => 1_000_000,
        (WalletChain::Tron, PreparedKind::TokenTransfer) => 15_000_000,
    };
    base.to_string()
}

/// Parse an `0x`-prefixed hex quantity, as every EVM JSON-RPC result encodes
/// integers.
///
/// `u128` rather than a 256-bit type. Nothing this wallet reads from a node —
/// a nonce, a gas price, a gas limit, a wei balance — approaches 2^128, which
/// is about 3.4e20 ETH, and carrying `ethers-core` for a bignum that is never
/// exercised past 128 bits is the trade this port exists to stop making. A
/// value that genuinely did overflow is reported rather than truncated.
///
/// # Errors
///
/// A message naming the offending value if it is not hex, or does not fit.
pub fn hex_to_u128(hex_value: &str) -> Result<u128, String> {
    let trimmed = hex_value.trim();
    let normalized = trimmed.strip_prefix("0x").unwrap_or(trimmed);
    u128::from_str_radix(normalized, 16)
        .map_err(|e| format!("invalid hex quantity '{hex_value}': {e}"))
}

/// Render an integer the way an EVM JSON-RPC parameter expects it.
#[must_use]
pub fn u128_to_hex(value: u128) -> String {
    format!("0x{value:x}")
}

pub fn hex_to_bytes(value: &str) -> Result<Vec<u8>, String> {
    let trimmed = value.trim();
    let normalized = trimmed.strip_prefix("0x").unwrap_or(trimmed);
    hex::decode(normalized).map_err(|e| format!("invalid hex bytes '{value}': {e}"))
}
