//! The high-level x402 client (intercept 402, pay, retry) and the
//! lower-level `handle_402` / `handle_402_and_pay` entry points the HTTP
//! tool layer drives directly.

use base64::engine::{general_purpose::STANDARD as B64, Engine as _};
use log::{debug, warn};
use reqwest::header::HeaderMap;

use super::super::types::*;
use super::errors::X402Error;
use super::evm_payment::build_evm_payment;
use super::headers::{parse_402_headers, parse_settlement_response};
use super::solana_payment::{b58_to_32, build_solana_payment};
use super::LOG_PREFIX;

/// High-level x402 client. Wraps a `reqwest::Client` and knows how to
/// intercept 402 responses, build Solana payments, and retry transparently.
pub struct X402Client {
    http: reqwest::Client,
}

impl X402Client {
    pub fn new(http: reqwest::Client) -> Self {
        Self { http }
    }

    /// Send a request. If the server returns 402 with a `PAYMENT-REQUIRED`
    /// header, attempt to pay using the wallet's Solana key and retry.
    ///
    /// The signing key is no longer a parameter: derivation and signing both
    /// happen inside the loaded wallet module, so there is no key for a caller
    /// to hold or pass. The wallet's phrase is resolved here and handed over on
    /// a confidential call.
    ///
    /// `max_amount` — optional ceiling in atomic units; rejects challenges above
    ///                this to prevent runaway spending.
    pub async fn try_paid_request(
        &self,
        request: reqwest::Request,
        max_amount: Option<u64>,
    ) -> Result<reqwest::Response, X402Error> {
        let method = request.method().clone();
        let url = request.url().clone();
        let headers = request.headers().clone();
        let body_bytes = request
            .body()
            .and_then(|b| b.as_bytes())
            .map(|b| b.to_vec());

        debug!("{LOG_PREFIX} initial request {} {}", method, url);
        let response = self
            .http
            .execute(request)
            .await
            .map_err(X402Error::Transport)?;

        if response.status() != reqwest::StatusCode::PAYMENT_REQUIRED {
            return Ok(response);
        }

        let challenge = parse_402_headers(response.headers())?;
        debug!(
            "{LOG_PREFIX} got 402 challenge version={} accepts={}",
            challenge.x402_version,
            challenge.accepts.len()
        );

        let (requirement, chain) = challenge
            .best_exact_requirement()
            .ok_or_else(|| X402Error::NoPaymentOption)?;

        let amount: u64 = requirement.amount.parse().map_err(|e| {
            X402Error::Protocol(format!("invalid amount '{}': {e}", requirement.amount))
        })?;

        if let Some(cap) = max_amount {
            if amount > cap {
                return Err(X402Error::AmountExceedsCap {
                    requested: amount,
                    cap,
                });
            }
        }

        debug!(
            "{LOG_PREFIX} paying {} atomic units of {} to {} chain={:?} (fee_payer={:?})",
            amount,
            requirement.asset,
            requirement.pay_to,
            chain,
            requirement.fee_payer_pubkey(),
        );

        let payment = match chain {
            PaymentChain::Solana => {
                let (config, signing_secret, our_pubkey) = wallet_signer().await?;
                build_solana_payment(
                    &config,
                    &signing_secret,
                    our_pubkey,
                    &challenge,
                    requirement,
                )
                .await?
            }
            PaymentChain::Evm => build_evm_payment(&challenge, requirement).await?,
        };
        let encoded = B64.encode(serde_json::to_string(&payment).unwrap());

        let mut retry_req = self.http.request(method, url);
        for (key, value) in headers.iter() {
            retry_req = retry_req.header(key, value);
        }
        retry_req = retry_req.header(HEADER_PAYMENT_SIGNATURE, &encoded);
        if let Some(body) = body_bytes {
            retry_req = retry_req.body(body);
        }

        debug!("{LOG_PREFIX} retrying with payment proof");
        let paid_response = retry_req.send().await.map_err(X402Error::Transport)?;

        if let Some(receipt_header) = paid_response.headers().get(HEADER_PAYMENT_RESPONSE) {
            match parse_settlement_response(receipt_header.to_str().unwrap_or("")) {
                Ok(receipt) => {
                    if receipt.success {
                        debug!(
                            "{LOG_PREFIX} payment settled tx={} network={}",
                            receipt.transaction, receipt.network
                        );
                    } else {
                        warn!(
                            "{LOG_PREFIX} payment settlement failed reason={:?}",
                            receipt.error_reason
                        );
                    }
                }
                Err(e) => warn!("{LOG_PREFIX} could not parse settlement response: {e}"),
            }
        }

        Ok(paid_response)
    }
}

/// Standalone entry point — parse a 402 response's headers and return the
/// challenge with the index of the best payment option and its chain family.
pub fn handle_402(
    headers: &HeaderMap,
) -> Result<(PaymentRequired, usize, PaymentChain), X402Error> {
    let challenge = parse_402_headers(headers)?;
    // Prefer Solana (lower fees, faster finality), fall back to EVM
    let (idx, chain) = challenge
        .accepts
        .iter()
        .enumerate()
        .find(|(_, r)| r.scheme == "exact" && r.network.starts_with("solana:"))
        .map(|(i, _)| (i, PaymentChain::Solana))
        .or_else(|| {
            challenge
                .accepts
                .iter()
                .enumerate()
                .find(|(_, r)| r.scheme == "exact" && r.network.starts_with("eip155:"))
                .map(|(i, _)| (i, PaymentChain::Evm))
        })
        .ok_or(X402Error::NoPaymentOption)?;
    Ok((challenge, idx, chain))
}

/// Build a payment and return the encoded header value ready to attach.
/// Separated from `try_paid_request` so callers that manage their own HTTP
/// layer can still use the payment construction.
pub async fn try_paid_request(
    challenge: &PaymentRequired,
    requirement: &PaymentRequirements,
) -> Result<String, X402Error> {
    let chain = if requirement.network.starts_with("eip155:") {
        PaymentChain::Evm
    } else {
        PaymentChain::Solana
    };
    let payment = match chain {
        PaymentChain::Solana => {
            let (config, signing_secret, our_pubkey) = wallet_signer().await?;
            build_solana_payment(&config, &signing_secret, our_pubkey, challenge, requirement)
                .await?
        }
        PaymentChain::Evm => build_evm_payment(challenge, requirement).await?,
    };
    let json = serde_json::to_string(&payment)
        .map_err(|e| X402Error::Protocol(format!("serialize payment: {e}")))?;
    Ok(B64.encode(json))
}

/// Result of a successful x402 payment retry — the payment header value and
/// metadata for the ledger.
pub struct X402PaymentResult {
    pub header_value: String,
    pub amount_atomic: u64,
    pub asset: String,
    pub recipient: String,
    pub network: String,
    pub url: String,
}

/// End-to-end 402 handler for the HTTP tool layer. Given a 402 response's
/// headers and the original URL:
///
/// 1. Parses the PAYMENT-REQUIRED challenge
/// 2. Checks the spending budget
/// 3. Derives the wallet's signing key (Solana preferred, EVM fallback)
/// 4. Builds a partially-signed payment transaction
/// 5. Returns the encoded PAYMENT-SIGNATURE header value
///
/// The caller retries the original request with this header attached and
/// records the payment outcome in the ledger.
pub async fn handle_402_and_pay(
    response_headers: &HeaderMap,
    request_url: &str,
) -> Result<X402PaymentResult, X402Error> {
    let (challenge, idx, chain) = handle_402(response_headers)?;
    let requirement = &challenge.accepts[idx];

    let amount: u64 = requirement.amount.parse().map_err(|e| {
        X402Error::Protocol(format!("invalid amount '{}': {e}", requirement.amount))
    })?;

    let budget_check =
        super::super::store::with_ledger(|l| l.check_budget(amount)).map_err(X402Error::Wallet)?;

    match budget_check {
        super::super::store::BudgetCheck::Allowed => {}
        super::super::store::BudgetCheck::ExceedsPerRequest { requested, cap } => {
            return Err(X402Error::AmountExceedsCap { requested, cap });
        }
        super::super::store::BudgetCheck::ExceedsDailyBudget { current, cap } => {
            return Err(X402Error::BudgetExceeded {
                period: "daily",
                current,
                cap,
            });
        }
        super::super::store::BudgetCheck::ExceedsMonthlyBudget { current, cap } => {
            return Err(X402Error::BudgetExceeded {
                period: "monthly",
                current,
                cap,
            });
        }
    }

    debug!(
        "{LOG_PREFIX} paying {} atomic {} to {} for {} chain={:?}",
        amount, requirement.asset, requirement.pay_to, request_url, chain
    );

    let payment = match chain {
        PaymentChain::Solana => {
            let (config, signing_secret, our_pubkey) = wallet_signer().await?;
            build_solana_payment(
                &config,
                &signing_secret,
                our_pubkey,
                &challenge,
                requirement,
            )
            .await?
        }
        PaymentChain::Evm => build_evm_payment(&challenge, requirement).await?,
    };

    let header_value = serde_json::to_string(&payment)
        .map(|json| B64.encode(json))
        .map_err(|e| X402Error::Protocol(format!("serialize payment: {e}")))?;

    Ok(X402PaymentResult {
        header_value,
        amount_atomic: amount,
        asset: requirement.asset.clone(),
        recipient: requirement.pay_to.clone(),
        network: requirement.network.clone(),
        url: request_url.to_string(),
    })
}

/// Derive the wallet's Solana ed25519 signing key from the encrypted mnemonic.
/// The phrase to sign a payment with, its config, and the wallet's public key.
///
/// Derivation happens in the loaded wallet module; this process never holds the
/// private key. The phrase is handed over on a confidential call, and only to a
/// module that has proved it is an artifact this build pinned.
async fn wallet_signer() -> Result<
    (
        crate::config::Config,
        tinywallet_bus::wire::SecretMaterial,
        [u8; 32],
    ),
    X402Error,
> {
    use crate::web3::wallet::WalletChain;

    let secret = crate::web3::wallet::secret_material(WalletChain::Solana)
        .await
        .map_err(|e| X402Error::Wallet(format!("wallet secret: {e}")))?;

    let config = crate::config::rpc::load_config_with_timeout()
        .await
        .map_err(|e| X402Error::Wallet(format!("load config: {e}")))?;

    let mnemonic =
        crate::security::encryption::rpc::decrypt_secret(&config, &secret.encrypted_mnemonic)
            .await
            .map_err(|e| X402Error::Wallet(format!("decrypt mnemonic: {e}")))?
            .value;

    let signing_secret = tinywallet_bus::wire::SecretMaterial {
        mnemonic,
        derivation_path: secret.derivation_path.clone(),
        chain: tinywallet_bus::Chain::Solana,
    };
    let account = crate::modules::wallet::derive_account(&config, &signing_secret)
        .await
        .map_err(|e| X402Error::Wallet(format!("derive account: {e}")))?;
    let pubkey = b58_to_32(&account.address)?;
    Ok((config, signing_secret, pubkey))
}
