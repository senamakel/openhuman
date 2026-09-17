//! x402 client operations — parse 402 challenges, build payment transactions
//! (Solana SPL or EVM ERC-20), sign, and retry with proof.
//!
//! Solana `exact` scheme layout:
//!  1. ComputeBudget::SetComputeUnitLimit
//!  2. ComputeBudget::SetComputeUnitPrice
//!  3. SPL Token `TransferChecked`
//!  4. (optional) SPL Memo with `extra.memo` or random nonce
//!
//! EVM `exact` scheme:
//!  EIP-3009 `transferWithAuthorization` signed by the wallet's EVM key.
//!  The facilitator submits the signed authorization on-chain.
//!
//! ## Module layout
//!
//! - [`errors`] — the client's error type.
//! - [`headers`] — parsing the challenge / settlement headers.
//! - [`solana_payment`] — Solana `exact` scheme transaction construction.
//! - [`evm_payment`] — EVM EIP-3009 payment construction.
//! - [`client`] — the high-level client and the `handle_402*` entry points.

mod client;
mod errors;
mod evm_payment;
mod headers;
mod solana_payment;

pub use client::{handle_402, handle_402_and_pay, try_paid_request, X402Client, X402PaymentResult};
pub use errors::X402Error;

#[cfg(test)]
pub(crate) use evm_payment::build_evm_payment_with_signer;

const LOG_PREFIX: &str = "[x402]";
