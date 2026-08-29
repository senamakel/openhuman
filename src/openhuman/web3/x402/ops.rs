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

use base64::engine::{general_purpose::STANDARD as B64, Engine as _};

use log::{debug, warn};
use reqwest::header::HeaderMap;
use sha2::{Digest, Sha256};

use super::types::*;

const LOG_PREFIX: &str = "[x402]";

/// Reasonable compute budget defaults for a single SPL TransferChecked.
const DEFAULT_COMPUTE_UNITS: u32 = 50_000;
const DEFAULT_COMPUTE_UNIT_PRICE: u64 = 1000; // micro-lamports per CU

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

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
        super::store::with_ledger(|l| l.check_budget(amount)).map_err(X402Error::Wallet)?;

    match budget_check {
        super::store::BudgetCheck::Allowed => {}
        super::store::BudgetCheck::ExceedsPerRequest { requested, cap } => {
            return Err(X402Error::AmountExceedsCap { requested, cap });
        }
        super::store::BudgetCheck::ExceedsDailyBudget { current, cap } => {
            return Err(X402Error::BudgetExceeded {
                period: "daily",
                current,
                cap,
            });
        }
        super::store::BudgetCheck::ExceedsMonthlyBudget { current, cap } => {
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
        crate::openhuman::config::Config,
        tinywallet_bus::wire::SecretMaterial,
        [u8; 32],
    ),
    X402Error,
> {
    use crate::openhuman::web3::wallet::WalletChain;

    let secret = crate::openhuman::web3::wallet::secret_material(WalletChain::Solana)
        .await
        .map_err(|e| X402Error::Wallet(format!("wallet secret: {e}")))?;

    let config = crate::openhuman::config::rpc::load_config_with_timeout()
        .await
        .map_err(|e| X402Error::Wallet(format!("load config: {e}")))?;

    let mnemonic = crate::openhuman::security::encryption::rpc::decrypt_secret(
        &config,
        &secret.encrypted_mnemonic,
    )
    .await
    .map_err(|e| X402Error::Wallet(format!("decrypt mnemonic: {e}")))?
    .value;

    let signing_secret = tinywallet_bus::wire::SecretMaterial {
        mnemonic,
        derivation_path: secret.derivation_path.clone(),
        chain: tinywallet_bus::Chain::Solana,
    };
    let account = crate::openhuman::modules::wallet::derive_account(&config, &signing_secret)
        .await
        .map_err(|e| X402Error::Wallet(format!("derive account: {e}")))?;
    let pubkey = b58_to_32(&account.address)?;
    Ok((config, signing_secret, pubkey))
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum X402Error {
    Transport(reqwest::Error),
    NoPaymentHeader,
    NoPaymentOption,
    AmountExceedsCap {
        requested: u64,
        cap: u64,
    },
    BudgetExceeded {
        period: &'static str,
        current: u64,
        cap: u64,
    },
    Protocol(String),
    Wallet(String),
}

impl std::fmt::Display for X402Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Transport(e) => write!(f, "x402 transport: {e}"),
            Self::NoPaymentHeader => write!(f, "402 response missing PAYMENT-REQUIRED header"),
            Self::NoPaymentOption => {
                write!(
                    f,
                    "no supported payment option (Solana exact or EVM exact) in 402 challenge"
                )
            }
            Self::AmountExceedsCap { requested, cap } => {
                write!(f, "x402 amount {requested} exceeds per-request cap {cap}")
            }
            Self::BudgetExceeded {
                period,
                current,
                cap,
            } => {
                write!(
                    f,
                    "x402 {period} budget exceeded: {current}/{cap} atomic units"
                )
            }
            Self::Protocol(msg) => write!(f, "x402 protocol: {msg}"),
            Self::Wallet(msg) => write!(f, "x402 wallet: {msg}"),
        }
    }
}

impl std::error::Error for X402Error {}

// ---------------------------------------------------------------------------
// Header parsing
// ---------------------------------------------------------------------------

fn parse_402_headers(headers: &HeaderMap) -> Result<PaymentRequired, X402Error> {
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

fn parse_settlement_response(b64_str: &str) -> Result<SettlementResponse, String> {
    let json_bytes = B64
        .decode(b64_str.trim())
        .map_err(|e| format!("PAYMENT-RESPONSE base64 decode: {e}"))?;
    serde_json::from_slice(&json_bytes).map_err(|e| format!("PAYMENT-RESPONSE JSON parse: {e}"))
}

// ---------------------------------------------------------------------------
// Solana transaction construction
// ---------------------------------------------------------------------------

/// Build a partially-signed Solana transaction for the `exact` scheme.
///
/// Layout:
///   account_keys[0] = fee_payer (facilitator) — signer, writable
///   account_keys[1] = our_pubkey (transfer authority) — signer, writable
///   account_keys[2] = src_ata — writable
///   account_keys[3] = dst_ata — writable
///   account_keys[4] = mint — readonly
///   account_keys[5] = token_program — readonly
///   account_keys[6] = compute_budget_program — readonly
///   account_keys[7] = memo_program — readonly (if memo present)
///
/// Instructions:
///   0. SetComputeUnitLimit(DEFAULT_COMPUTE_UNITS)
///   1. SetComputeUnitPrice(DEFAULT_COMPUTE_UNIT_PRICE)
///   2. TransferChecked { amount, decimals=6 }
///   3. Memo (if extra.memo set, otherwise random 16-byte hex nonce)
async fn build_solana_payment(
    config: &crate::openhuman::config::Config,
    signing_secret: &tinywallet_bus::wire::SecretMaterial,
    our_pubkey: [u8; 32],
    challenge: &PaymentRequired,
    req: &PaymentRequirements,
) -> Result<PaymentPayload, X402Error> {
    let amount: u64 = req
        .amount
        .parse()
        .map_err(|e| X402Error::Protocol(format!("invalid amount '{}': {e}", req.amount)))?;

    let fee_payer = req
        .fee_payer_pubkey()
        .ok_or_else(|| X402Error::Protocol("no fee_payer in payment requirements".into()))?;
    let fee_payer_bytes = b58_to_32(fee_payer)?;
    let pay_to_bytes = b58_to_32(&req.pay_to)?;
    let mint_bytes = b58_to_32(&req.asset)?;

    let token_program = b58_to_32(SPL_TOKEN_PROGRAM)?;
    let compute_budget = b58_to_32(COMPUTE_BUDGET_PROGRAM)?;
    let memo_program = b58_to_32(SPL_MEMO_PROGRAM)?;

    let src_ata = derive_ata(&our_pubkey, &mint_bytes, &token_program)?;
    let dst_ata = derive_ata(&pay_to_bytes, &mint_bytes, &token_program)?;

    let memo_data = req
        .memo_value()
        .map(|m| m.as_bytes().to_vec())
        .unwrap_or_else(random_memo_nonce);

    // -- account keys (order matters) --
    let account_keys: Vec<[u8; 32]> = vec![
        fee_payer_bytes, // 0: fee payer (signer, writable)
        our_pubkey,      // 1: transfer authority (signer, writable)
        src_ata,         // 2: source ATA (writable)
        dst_ata,         // 3: destination ATA (writable)
        mint_bytes,      // 4: mint (readonly)
        token_program,   // 5: SPL Token program (readonly)
        compute_budget,  // 6: Compute Budget program (readonly)
        memo_program,    // 7: SPL Memo program (readonly)
    ];

    // header: [num_required_sigs, num_readonly_signed, num_readonly_unsigned]
    // 2 signers (fee_payer + us), 0 readonly signed, 4 readonly unsigned
    // (mint, token_program, compute_budget, memo_program)
    let header = [2u8, 0u8, 4u8];

    // -- instructions --
    let set_cu_limit = build_set_compute_unit_limit(6, DEFAULT_COMPUTE_UNITS);
    let set_cu_price = build_set_compute_unit_price(6, DEFAULT_COMPUTE_UNIT_PRICE);
    let transfer_checked = build_transfer_checked(
        5, // token_program index
        2, // src_ata index
        4, // mint index
        3, // dst_ata index
        1, // authority (our_pubkey) index
        amount, 6, // USDC decimals
    );
    let memo = build_memo(7, &memo_data);

    let instructions = vec![set_cu_limit, set_cu_price, transfer_checked, memo];

    // -- fetch recent blockhash --
    let blockhash = fetch_recent_blockhash_for_x402().await?;

    // -- encode message --
    let message = encode_legacy_message(&header, &account_keys, &blockhash, &instructions);

    // -- build wire: 2 signature slots, sign only ours (index 1) --
    let mut wire = Vec::with_capacity(1 + 128 + message.len());
    wire.extend(encode_shortvec(2)); // 2 required signatures
    wire.extend([0u8; 64]); // slot 0: fee_payer (left zeroed for facilitator)

    // Signed in the module: the phrase goes over a confidential call and the
    // private key is never assembled in this process.
    let signature = crate::openhuman::modules::wallet::sign_message(
        config,
        signing_secret,
        &message,
        tinywallet_bus::wire::Scheme::Ed25519,
    )
    .await
    .map_err(|e| X402Error::Wallet(format!("sign payment: {e}")))?;
    let tinywallet_bus::wire::Signature::Ed25519 { signature_hex } = signature else {
        return Err(X402Error::Wallet(
            "the wallet module returned a non-ed25519 signature".to_string(),
        ));
    };
    let sig_bytes = hex_to_32_bytes_64(&signature_hex)?;
    wire.extend(sig_bytes); // slot 1: our signature
    wire.extend(&message);

    let tx_b64 = B64.encode(&wire);
    debug!(
        "{LOG_PREFIX} built payment tx {} bytes, amount={amount} asset={}",
        wire.len(),
        req.asset
    );

    Ok(PaymentPayload {
        x402_version: X402_VERSION,
        resource: Some(challenge.resource.clone()),
        accepted: req.clone(),
        payload: PaymentProof::Solana(SolanaPaymentProof {
            transaction: tx_b64,
        }),
        extensions: serde_json::Map::new(),
    })
}

// ---------------------------------------------------------------------------
// EVM payment construction (EIP-3009 transferWithAuthorization)
// ---------------------------------------------------------------------------

/// Build an EVM payment using EIP-3009 `transferWithAuthorization`.
/// Signs the typed data with the wallet's EVM key and returns the proof
/// for the facilitator to submit on-chain.
async fn build_evm_payment(
    challenge: &PaymentRequired,
    req: &PaymentRequirements,
) -> Result<PaymentPayload, X402Error> {
    let (config, signing_secret, from_address) = evm_signer().await?;
    let authorization = evm_payment_authorization(&from_address, req)?;

    // Signed in the wallet module over the prehashed EIP-712 digest. This
    // process never holds the EVM key.
    let signature = crate::openhuman::modules::wallet::sign_message(
        &config,
        &signing_secret,
        &authorization.digest,
        tinywallet_bus::wire::Scheme::Secp256k1Prehash,
    )
    .await
    .map_err(|e| X402Error::Wallet(format!("sign EIP-3009: {e}")))?;
    let tinywallet_bus::wire::Signature::Secp256k1 {
        rs_hex,
        recovery_id,
    } = signature
    else {
        return Err(X402Error::Wallet(
            "the wallet module returned a non-secp256k1 signature".to_string(),
        ));
    };
    let rs = hex::decode(&rs_hex)
        .map_err(|e| X402Error::Wallet(format!("invalid signature hex: {e}")))?;
    if rs.len() != 64 {
        return Err(X402Error::Wallet(
            "the wallet module returned a malformed signature".to_string(),
        ));
    }
    let mut sig_bytes = [0u8; 65];
    sig_bytes[..64].copy_from_slice(&rs);
    sig_bytes[64] = recovery_id
        .checked_add(27)
        .ok_or_else(|| X402Error::Wallet("recovery id out of range".to_string()))?;

    evm_payment_payload(&authorization, sig_bytes, &from_address, challenge, req)
}

/// The EIP-712 digest to sign, and the fields the payload needs alongside it.
///
/// Split out from signing so that production (which signs in the wallet module)
/// and the tests (which sign locally, to check the construction against a fixed
/// vector) share one implementation of the part that can be wrong. Only *who
/// holds the key* differs between them.
pub(crate) struct EvmPaymentAuthorization {
    /// The 32-byte EIP-712 digest.
    pub digest: [u8; 32],
    /// The EIP-3009 nonce, echoed into the payload.
    pub nonce: [u8; 32],
    valid_after_secs: u64,
    valid_before_secs: u64,
}

/// Compute the EIP-3009 authorization and its EIP-712 digest.
pub(crate) fn evm_payment_authorization(
    from_address: &str,
    req: &PaymentRequirements,
) -> Result<EvmPaymentAuthorization, X402Error> {
    use tinywallet_bus::eip712;

    let chain_id = req
        .evm_chain_id()
        .ok_or_else(|| X402Error::Protocol(format!("not an EVM network: {}", req.network)))?;

    let amount = eip712::u256_from_decimal(&req.amount)
        .map_err(|e| X402Error::Protocol(format!("invalid amount '{}': {e}", req.amount)))?;

    let from_bytes = evm_address_bytes(from_address)?;
    let pay_to = evm_address_bytes(&req.pay_to)?;
    let token_address = evm_address_bytes(&req.asset)?;

    // EIP-3009 parameters
    let valid_after = eip712::u256_from_u64(0);
    let valid_before_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .saturating_add(req.max_timeout_seconds);
    let valid_before = eip712::u256_from_u64(valid_before_secs);

    // Random nonce for EIP-3009
    let nonce = {
        let mut hasher = Sha256::new();
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        hasher.update(ts.to_le_bytes());
        hasher.update(std::process::id().to_le_bytes());
        let hash: [u8; 32] = hasher.finalize().into();
        hash
    };

    // EIP-712 typed data for `transferWithAuthorization`
    let domain_name = req
        .extra
        .as_ref()
        .and_then(|e| e.name.as_deref())
        .unwrap_or("USD Coin");
    let domain_version = req
        .extra
        .as_ref()
        .and_then(|e| e.version.as_deref())
        .unwrap_or("2");
    let domain_separator =
        eip712::domain_separator(token_address, chain_id, domain_name, domain_version);
    let struct_hash = eip712::transfer_with_authorization_hash(
        from_bytes,
        pay_to,
        amount,
        valid_after,
        valid_before,
        nonce,
    );
    let digest = eip712::signing_digest(domain_separator, struct_hash);

    Ok(EvmPaymentAuthorization {
        digest,
        nonce,
        valid_after_secs: 0,
        valid_before_secs,
    })
}

/// Assemble the payload from an authorization and its signature.
///
/// An EIP-712 signature is `r ‖ s ‖ v` where `v` is the recovery id offset by
/// 27 — not EIP-155's chain-mixed `v`, because typed data is not a transaction.
pub(crate) fn evm_payment_payload(
    authorization: &EvmPaymentAuthorization,
    sig_bytes: [u8; 65],
    from_address: &str,
    challenge: &PaymentRequired,
    req: &PaymentRequirements,
) -> Result<PaymentPayload, X402Error> {
    let chain_id = req
        .evm_chain_id()
        .ok_or_else(|| X402Error::Protocol(format!("not an EVM network: {}", req.network)))?;
    let valid_after = authorization.valid_after_secs;
    let valid_before = authorization.valid_before_secs;
    let nonce = authorization.nonce;

    let sig_hex = format!("0x{}", hex::encode(sig_bytes));
    let nonce_hex = format!("0x{}", hex::encode(nonce));

    debug!(
        "{LOG_PREFIX} built EVM payment chain_id={chain_id} amount={} asset={} from={} to={}",
        req.amount, req.asset, from_address, req.pay_to
    );

    Ok(PaymentPayload {
        x402_version: X402_VERSION,
        resource: Some(challenge.resource.clone()),
        accepted: req.clone(),
        payload: PaymentProof::Evm(EvmPaymentProof {
            signature: sig_hex,
            authorization: EvmAuthorization {
                from: from_address.to_string(),
                to: req.pay_to.clone(),
                value: req.amount.clone(),
                valid_after: valid_after.to_string(),
                valid_before: valid_before.to_string(),
                nonce: nonce_hex,
            },
        }),
        extensions: serde_json::Map::new(),
    })
}

/// Resolve the EVM account an x402 payment will be signed as.
///
/// Returns the config, the [`SecretMaterial`](tinywallet_bus::wire::SecretMaterial)
/// the signing calls take, and the checksummed address that material controls.
///
/// # Where the key is, and where it is not
///
/// **No private key is derived in this process.** The address comes back from
/// `modules::wallet::derive_account` — a confidential call into the loaded
/// `tinywallet` module, which derives, answers with public data only, and wipes
/// its copy of the phrase before returning. The signature is produced the same
/// way, by `modules::wallet::sign_message`. This binary links no derivation
/// stack at all: it takes `tinywallet-bus`, the wire contract, and `key` is one
/// of the gates that deliberately stayed in the root crate.
///
/// What *does* live in this process is the decrypted **mnemonic**, held in the
/// returned `SecretMaterial` for as long as a caller holds it and sent across
/// the bus on each confidential call. That is the exposure to reason about
/// here; a derived private key is not.
///
/// Deriving the address rather than assuming one is what makes an x402 payment
/// signed by exactly the account the wallet reports.
async fn evm_signer() -> Result<
    (
        crate::openhuman::config::Config,
        tinywallet_bus::wire::SecretMaterial,
        String,
    ),
    X402Error,
> {
    use crate::openhuman::web3::wallet::WalletChain;

    let secret = crate::openhuman::web3::wallet::secret_material(WalletChain::Evm)
        .await
        .map_err(|e| X402Error::Wallet(format!("wallet secret: {e}")))?;

    let config = crate::openhuman::config::rpc::load_config_with_timeout()
        .await
        .map_err(|e| X402Error::Wallet(format!("load config: {e}")))?;

    let mnemonic = crate::openhuman::security::encryption::rpc::decrypt_secret(
        &config,
        &secret.encrypted_mnemonic,
    )
    .await
    .map_err(|e| X402Error::Wallet(format!("decrypt mnemonic: {e}")))?
    .value;

    let signing_secret = tinywallet_bus::wire::SecretMaterial {
        mnemonic,
        derivation_path: secret.derivation_path.clone(),
        chain: tinywallet_bus::Chain::Evm,
    };
    let account = crate::openhuman::modules::wallet::derive_account(&config, &signing_secret)
        .await
        .map_err(|e| X402Error::Wallet(format!("derive EVM signer: {e}")))?;

    Ok((config, signing_secret, account.address))
}

/// Sign an EIP-712 digest locally. Test-only.
///
/// Production signs in the wallet module; this exists so the payment
/// construction can be checked against a fixed vector without a broker. It is
/// the only remaining local use of a private key in this domain, and it is
/// compiled out of the shipped binary.
#[cfg(test)]
pub(crate) fn sign_evm_digest_locally(
    secret: &[u8],
    digest: &[u8; 32],
) -> Result<[u8; 65], X402Error> {
    let key = k256::ecdsa::SigningKey::from_slice(secret)
        .map_err(|_| X402Error::Wallet("derived EVM key is unusable".to_string()))?;
    let (signature, recovery_id) = key
        .sign_prehash_recoverable(digest)
        .map_err(|e| X402Error::Wallet(format!("EVM sign EIP-3009: {e}")))?;
    let mut sig_bytes = [0u8; 65];
    sig_bytes[..64].copy_from_slice(&signature.to_bytes());
    sig_bytes[64] = recovery_id.to_byte() + 27;
    Ok(sig_bytes)
}

/// The construction the tests drive: authorize, sign locally, assemble.
#[cfg(test)]
pub(crate) fn build_evm_payment_with_signer(
    secret: &[u8],
    from_address: &str,
    challenge: &PaymentRequired,
    req: &PaymentRequirements,
) -> Result<PaymentPayload, X402Error> {
    let authorization = evm_payment_authorization(from_address, req)?;
    let sig_bytes = sign_evm_digest_locally(secret, &authorization.digest)?;
    evm_payment_payload(&authorization, sig_bytes, from_address, challenge, req)
}

/// The 20 raw bytes of an EVM address.
fn evm_address_bytes(address: &str) -> Result<[u8; 20], X402Error> {
    let validated = tinywallet_bus::address::evm::validate(address)
        .map_err(|e| X402Error::Protocol(format!("invalid EVM address '{address}': {e}")))?;
    let body = validated.strip_prefix("0x").unwrap_or(&validated);
    let decoded = hex::decode(body)
        .map_err(|_| X402Error::Protocol(format!("non-hex EVM address '{address}'")))?;
    decoded
        .try_into()
        .map_err(|_| X402Error::Protocol(format!("truncated EVM address '{address}'")))
}

// ---------------------------------------------------------------------------
// Solana wire-format helpers (mirrors wallet/chains/solana.rs primitives)
// ---------------------------------------------------------------------------

fn b58_to_32(addr: &str) -> Result<[u8; 32], X402Error> {
    let v = bs58::decode(addr.trim())
        .into_vec()
        .map_err(|e| X402Error::Protocol(format!("invalid base58 '{addr}': {e}")))?;
    if v.len() != 32 {
        return Err(X402Error::Protocol(format!(
            "expected 32-byte key, got {} for '{addr}'",
            v.len()
        )));
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&v);
    Ok(out)
}

fn derive_ata(
    owner: &[u8; 32],
    mint: &[u8; 32],
    token_program: &[u8; 32],
) -> Result<[u8; 32], X402Error> {
    let ata_program = b58_to_32("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL")?;
    for bump in (0u8..=255).rev() {
        let mut hasher = Sha256::new();
        hasher.update(owner);
        hasher.update(token_program);
        hasher.update(mint);
        hasher.update([bump]);
        hasher.update(ata_program);
        hasher.update(b"ProgramDerivedAddress");
        let candidate: [u8; 32] = hasher.finalize().into();
        if curve25519_dalek::edwards::CompressedEdwardsY(candidate)
            .decompress()
            .is_none()
        {
            return Ok(candidate);
        }
    }
    Err(X402Error::Protocol("ATA PDA derivation failed".into()))
}

fn encode_shortvec(value: u16) -> Vec<u8> {
    let mut out = Vec::new();
    let mut v = value as u32;
    loop {
        let mut byte = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(byte);
            return out;
        }
        byte |= 0x80;
        out.push(byte);
    }
}

struct Instruction {
    program_id_index: u8,
    accounts: Vec<u8>,
    data: Vec<u8>,
}

fn build_set_compute_unit_limit(program_idx: u8, units: u32) -> Instruction {
    let mut data = vec![2u8]; // discriminator
    data.extend(units.to_le_bytes());
    Instruction {
        program_id_index: program_idx,
        accounts: vec![],
        data,
    }
}

fn build_set_compute_unit_price(program_idx: u8, micro_lamports: u64) -> Instruction {
    let mut data = vec![3u8]; // discriminator
    data.extend(micro_lamports.to_le_bytes());
    Instruction {
        program_id_index: program_idx,
        accounts: vec![],
        data,
    }
}

fn build_transfer_checked(
    token_program_idx: u8,
    src_idx: u8,
    mint_idx: u8,
    dst_idx: u8,
    authority_idx: u8,
    amount: u64,
    decimals: u8,
) -> Instruction {
    let mut data = vec![12u8]; // SPL Token: TransferChecked = 12
    data.extend(amount.to_le_bytes());
    data.push(decimals);
    Instruction {
        program_id_index: token_program_idx,
        accounts: vec![src_idx, mint_idx, dst_idx, authority_idx],
        data,
    }
}

fn build_memo(program_idx: u8, memo_data: &[u8]) -> Instruction {
    Instruction {
        program_id_index: program_idx,
        accounts: vec![],
        data: memo_data.to_vec(),
    }
}

fn encode_instruction(ins: &Instruction) -> Vec<u8> {
    let mut out = Vec::new();
    out.push(ins.program_id_index);
    out.extend(encode_shortvec(ins.accounts.len() as u16));
    out.extend(&ins.accounts);
    out.extend(encode_shortvec(ins.data.len() as u16));
    out.extend(&ins.data);
    out
}

fn encode_legacy_message(
    header: &[u8; 3],
    account_keys: &[[u8; 32]],
    recent_blockhash: &[u8; 32],
    instructions: &[Instruction],
) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend(header);
    out.extend(encode_shortvec(account_keys.len() as u16));
    for key in account_keys {
        out.extend(key);
    }
    out.extend(recent_blockhash);
    out.extend(encode_shortvec(instructions.len() as u16));
    for ins in instructions {
        out.extend(encode_instruction(ins));
    }
    out
}

fn random_memo_nonce() -> Vec<u8> {
    use std::time::{SystemTime, UNIX_EPOCH};
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let mut hasher = Sha256::new();
    hasher.update(ts.to_le_bytes());
    hasher.update(std::process::id().to_le_bytes());
    let hash: [u8; 32] = hasher.finalize().into();
    hex::encode(&hash[..16]).into_bytes()
}

async fn fetch_recent_blockhash_for_x402() -> Result<[u8; 32], X402Error> {
    use crate::openhuman::web3::wallet::WalletChain;

    #[derive(serde::Deserialize)]
    struct BlockhashResponse {
        value: BlockhashValue,
    }
    #[derive(serde::Deserialize)]
    struct BlockhashValue {
        blockhash: String,
    }

    let result: BlockhashResponse = crate::openhuman::web3::wallet::rpc::rpc_call(
        WalletChain::Solana,
        "getLatestBlockhash",
        serde_json::json!([{"commitment": "finalized"}]),
    )
    .await
    .map_err(|e| X402Error::Wallet(format!("fetch blockhash: {e}")))?;

    b58_to_32(&result.value.blockhash)
}

/// Decode a 64-byte signature returned as lowercase hex by the wallet module.
fn hex_to_32_bytes_64(value: &str) -> Result<[u8; 64], X402Error> {
    if value.len() != 128 {
        return Err(X402Error::Wallet(
            "the wallet module returned a malformed signature".to_string(),
        ));
    }
    let mut out = [0u8; 64];
    for (index, slot) in out.iter_mut().enumerate() {
        *slot = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16)
            .map_err(|e| X402Error::Wallet(format!("invalid signature hex: {e}")))?;
    }
    Ok(out)
}
