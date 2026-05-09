//! Core-owned wallet onboarding metadata and derived account visibility.

mod ops;
mod schemas;

pub use ops::{
    setup, status, WalletAccount, WalletChain, WalletSetupParams, WalletSetupSource, WalletStatus,
};
pub use schemas::{
    all_controller_schemas, all_registered_controllers, all_wallet_controller_schemas,
    all_wallet_registered_controllers, schemas, wallet_schemas,
};
