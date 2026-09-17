//! Wire and domain types for wallet setup, status, and secret material.

use serde::{Deserialize, Serialize};

/// Error message returned when the wallet has not been set up yet.
///
/// This is an expected user-state (the user simply has not created a wallet),
/// not an internal failure. Downstream boundaries that surface this condition
/// match against this constant to
/// classify it as `expected_user_state` so it stays out of Sentry. Keep it a
/// shared constant so the producer here and any classifier cannot drift apart.
pub const WALLET_NOT_CONFIGURED_MESSAGE: &str = "wallet is not configured; run wallet setup first";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WalletChain {
    Evm,
    Btc,
    Solana,
    Tron,
}

impl WalletChain {
    pub(super) const ALL: [Self; 4] = [Self::Evm, Self::Btc, Self::Solana, Self::Tron];

    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Evm => "evm",
            Self::Btc => "btc",
            Self::Solana => "solana",
            Self::Tron => "tron",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WalletSetupSource {
    Generated,
    Imported,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WalletAccount {
    pub chain: WalletChain,
    pub address: String,
    pub derivation_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WalletSetupParams {
    pub consent_granted: bool,
    pub source: WalletSetupSource,
    pub mnemonic_word_count: u8,
    #[serde(default)]
    pub encrypted_mnemonic: Option<String>,
    pub accounts: Vec<WalletAccount>,
    /// When `true`, allows overwriting an existing wallet.
    /// Requires explicit user confirmation in the frontend.
    /// Defaults to `false` — a guard against silent overwrites.
    #[serde(default)]
    pub force: bool,
}

/// Persisted on-disk (and/or keychain-backed) representation of wallet state.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(super) struct StoredWalletState {
    pub consent_granted: bool,
    pub source: WalletSetupSource,
    pub mnemonic_word_count: u8,
    #[serde(default)]
    pub encrypted_mnemonic: Option<String>,
    pub accounts: Vec<WalletAccount>,
    pub updated_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WalletSecretMaterial {
    pub encrypted_mnemonic: String,
    pub derivation_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WalletStatus {
    pub configured: bool,
    pub onboarding_completed: bool,
    pub consent_granted: bool,
    pub secret_stored: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<WalletSetupSource>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mnemonic_word_count: Option<u8>,
    pub accounts: Vec<WalletAccount>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at_ms: Option<u64>,
}

/// Result returned by `reveal_recovery_phrase`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RevealRecoveryPhraseResult {
    pub phrase: String,
    pub word_count: usize,
}
