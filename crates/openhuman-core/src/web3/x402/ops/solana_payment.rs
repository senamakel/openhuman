//! Solana `exact` scheme payment construction: a partially-signed legacy
//! transaction carrying ComputeBudget + SPL `TransferChecked` (+ optional
//! Memo) instructions, encoded by hand — no `solana-sdk` dependency.
//!
//! Layout:
//!   account_keys[0] = fee_payer (facilitator) — signer, writable
//!   account_keys[1] = our_pubkey (transfer authority) — signer, writable
//!   account_keys[2] = src_ata — writable
//!   account_keys[3] = dst_ata — writable
//!   account_keys[4] = mint — readonly
//!   account_keys[5] = token_program — readonly
//!   account_keys[6] = compute_budget_program — readonly
//!   account_keys[7] = memo_program — readonly (if memo present)
//!
//! Instructions:
//!   0. SetComputeUnitLimit(DEFAULT_COMPUTE_UNITS)
//!   1. SetComputeUnitPrice(DEFAULT_COMPUTE_UNIT_PRICE)
//!   2. TransferChecked { amount, decimals=6 }
//!   3. Memo (if extra.memo set, otherwise random 16-byte hex nonce)

use base64::engine::{general_purpose::STANDARD as B64, Engine as _};
use log::debug;
use sha2::{Digest, Sha256};

use super::super::types::*;
use super::errors::X402Error;
use super::LOG_PREFIX;

/// Reasonable compute budget defaults for a single SPL TransferChecked.
const DEFAULT_COMPUTE_UNITS: u32 = 50_000;
const DEFAULT_COMPUTE_UNIT_PRICE: u64 = 1000; // micro-lamports per CU

/// Build a partially-signed Solana transaction for the `exact` scheme.
pub(super) async fn build_solana_payment(
    config: &crate::config::Config,
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
    let signature = crate::modules::wallet::sign_message(
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
// Solana wire-format helpers (mirrors wallet/chains/solana.rs primitives)
// ---------------------------------------------------------------------------

pub(super) fn b58_to_32(addr: &str) -> Result<[u8; 32], X402Error> {
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
    use crate::web3::wallet::WalletChain;

    #[derive(serde::Deserialize)]
    struct BlockhashResponse {
        value: BlockhashValue,
    }
    #[derive(serde::Deserialize)]
    struct BlockhashValue {
        blockhash: String,
    }

    let result: BlockhashResponse = crate::web3::wallet::rpc::rpc_call(
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
