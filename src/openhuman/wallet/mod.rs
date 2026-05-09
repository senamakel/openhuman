//! Core-owned wallet onboarding metadata, derived account visibility, and
//! the agent-facing execution surface (balances, transfers, swaps,
//! contract calls). See [`execution`] for the prepare/confirm/execute flow.

mod execution;
mod ops;
mod schemas;

pub use execution::{
    balances, chain_status, execute_prepared, prepare_contract_call, prepare_swap,
    prepare_transfer, supported_assets, BalanceInfo, ChainStatus, ExecutePreparedParams,
    PrepareContractCallParams, PrepareSwapParams, PrepareTransferParams, PreparedKind,
    PreparedStatus, PreparedTransaction, ProviderStatus, ReadyToSign, SupportedAsset,
};
pub use ops::{
    setup, status, WalletAccount, WalletChain, WalletSetupParams, WalletSetupSource, WalletStatus,
};
pub use schemas::{
    all_controller_schemas, all_registered_controllers, all_wallet_controller_schemas,
    all_wallet_registered_controllers, schemas, wallet_schemas,
};
