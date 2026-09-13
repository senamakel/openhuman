//! EVM `exact` scheme payment construction: EIP-3009
//! `transferWithAuthorization`, signed over its EIP-712 digest by the
//! wallet's EVM key. The facilitator submits the signed authorization
//! on-chain.

use log::debug;
use sha2::{Digest, Sha256};

use super::super::types::*;
use super::errors::X402Error;
use super::LOG_PREFIX;

/// Build an EVM payment using EIP-3009 `transferWithAuthorization`.
/// Signs the typed data with the wallet's EVM key and returns the proof
/// for the facilitator to submit on-chain.
pub(super) async fn build_evm_payment(
    challenge: &PaymentRequired,
    req: &PaymentRequirements,
) -> Result<PaymentPayload, X402Error> {
    let (config, signing_secret, from_address) = evm_signer().await?;
    let authorization = evm_payment_authorization(&from_address, req)?;

    // Signed in the wallet module over the prehashed EIP-712 digest. This
    // process never holds the EVM key.
    let signature = crate::modules::wallet::sign_message(
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
pub(super) async fn evm_signer() -> Result<
    (
        crate::config::Config,
        tinywallet_bus::wire::SecretMaterial,
        String,
    ),
    X402Error,
> {
    use crate::web3::wallet::WalletChain;

    let secret = crate::web3::wallet::secret_material(WalletChain::Evm)
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
        chain: tinywallet_bus::Chain::Evm,
    };
    let account = crate::modules::wallet::derive_account(&config, &signing_secret)
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
