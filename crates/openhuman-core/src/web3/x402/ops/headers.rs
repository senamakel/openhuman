//! Parsing the `PAYMENT-REQUIRED` challenge and `PAYMENT-RESPONSE` settlement
//! headers, both base64-encoded JSON.

use base64::engine::{general_purpose::STANDARD as B64, Engine as _};
use log::warn;
use reqwest::header::HeaderMap;

use super::super::types::*;
use super::errors::X402Error;
use super::LOG_PREFIX;

pub(super) fn parse_402_headers(headers: &HeaderMap) -> Result<PaymentRequired, X402Error> {
    let raw = headers
        .get(HEADER_PAYMENT_REQUIRED)
        .or_else(|| headers.get(HEADER_PAYMENT_REQUIRED_V1))
        .ok_or(X402Error::NoPaymentHeader)?;
    let b64_str = raw.to_str().map_err(|e| {
        X402Error::Protocol(format!("PAYMENT-REQUIRED header not valid UTF-8: {e}"))
    })?;
    let json_bytes = B64
        .decode(b64_str.trim())
        .map_err(|e| X402Error::Protocol(format!("PAYMENT-REQUIRED base64 decode: {e}")))?;
    let challenge: PaymentRequired = serde_json::from_slice(&json_bytes)
        .map_err(|e| X402Error::Protocol(format!("PAYMENT-REQUIRED JSON parse: {e}")))?;
    if challenge.x402_version != X402_VERSION {
        warn!(
            "{LOG_PREFIX} unexpected x402 version {} (expected {X402_VERSION})",
            challenge.x402_version
        );
    }
    Ok(challenge)
}

pub(super) fn parse_settlement_response(b64_str: &str) -> Result<SettlementResponse, String> {
    let json_bytes = B64
        .decode(b64_str.trim())
        .map_err(|e| format!("PAYMENT-RESPONSE base64 decode: {e}"))?;
    serde_json::from_slice(&json_bytes).map_err(|e| format!("PAYMENT-RESPONSE JSON parse: {e}"))
}
