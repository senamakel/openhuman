use serde::Deserialize;
use serde_json::{Map, Value};

use crate::core::all::{ControllerFuture, RegisteredController};
use crate::core::{ControllerSchema, FieldSchema, TypeSchema};

use super::ops::{WalletAccount, WalletSetupParams, WalletSetupSource};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SetupWalletParams {
    consent_granted: bool,
    source: WalletSetupSource,
    mnemonic_word_count: u8,
    accounts: Vec<WalletAccount>,
}

pub fn all_controller_schemas() -> Vec<ControllerSchema> {
    all_wallet_controller_schemas()
}

pub fn all_registered_controllers() -> Vec<RegisteredController> {
    all_wallet_registered_controllers()
}

pub fn schemas(function: &str) -> ControllerSchema {
    wallet_schemas(function)
}

pub fn all_wallet_controller_schemas() -> Vec<ControllerSchema> {
    vec![wallet_schemas("status"), wallet_schemas("setup")]
}

pub fn all_wallet_registered_controllers() -> Vec<RegisteredController> {
    vec![
        RegisteredController {
            schema: wallet_schemas("status"),
            handler: handle_status,
        },
        RegisteredController {
            schema: wallet_schemas("setup"),
            handler: handle_setup,
        },
    ]
}

pub fn wallet_schemas(function: &str) -> ControllerSchema {
    match function {
        "status" => ControllerSchema {
            namespace: "wallet",
            function: "status",
            description: "Fetch core-owned local wallet metadata and onboarding status.",
            inputs: vec![],
            outputs: vec![FieldSchema {
                name: "result",
                ty: TypeSchema::Json,
                comment: "Wallet onboarding status plus safe multi-chain account metadata.",
                required: true,
            }],
        },
        "setup" => ControllerSchema {
            namespace: "wallet",
            function: "setup",
            description:
                "Persist local wallet consent and derived account metadata from the recovery phrase flow.",
            inputs: vec![
                required_json("consentGranted", "Whether the user explicitly consented to wallet setup."),
                required_json("source", "Whether the recovery phrase was generated or imported."),
                required_json(
                    "mnemonicWordCount",
                    "The number of words in the validated recovery phrase.",
                ),
                required_json(
                    "accounts",
                    "Exactly one derived account for each supported chain: EVM, BTC, Solana, and Tron.",
                ),
            ],
            outputs: vec![FieldSchema {
                name: "result",
                ty: TypeSchema::Json,
                comment: "Updated wallet status after saving the setup.",
                required: true,
            }],
        },
        _ => ControllerSchema {
            namespace: "wallet",
            function: "unknown",
            description: "Unknown wallet controller.",
            inputs: vec![],
            outputs: vec![FieldSchema {
                name: "error",
                ty: TypeSchema::String,
                comment: "Lookup error details.",
                required: true,
            }],
        },
    }
}

fn handle_status(_params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        crate::openhuman::wallet::status()
            .await?
            .into_cli_compatible_json()
    })
}

fn handle_setup(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let payload: SetupWalletParams = serde_json::from_value(Value::Object(params))
            .map_err(|e| format!("invalid params: {e}"))?;
        crate::openhuman::wallet::setup(WalletSetupParams {
            consent_granted: payload.consent_granted,
            source: payload.source,
            mnemonic_word_count: payload.mnemonic_word_count,
            accounts: payload.accounts,
        })
        .await?
        .into_cli_compatible_json()
    })
}

fn required_json(name: &'static str, comment: &'static str) -> FieldSchema {
    FieldSchema {
        name,
        ty: TypeSchema::Json,
        comment,
        required: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_schemas_returns_two() {
        assert_eq!(all_wallet_controller_schemas().len(), 2);
    }

    #[test]
    fn all_controllers_returns_two() {
        assert_eq!(all_wallet_registered_controllers().len(), 2);
    }

    #[test]
    fn status_schema_is_empty_input() {
        let schema = wallet_schemas("status");
        assert_eq!(schema.namespace, "wallet");
        assert_eq!(schema.function, "status");
        assert!(schema.inputs.is_empty());
    }

    #[test]
    fn setup_schema_requires_all_inputs() {
        let schema = wallet_schemas("setup");
        assert_eq!(schema.inputs.len(), 4);
        assert!(schema.inputs.iter().all(|field| field.required));
    }

    #[test]
    fn unknown_schema_maps_to_unknown() {
        let schema = wallet_schemas("wat");
        assert_eq!(schema.function, "unknown");
    }
}
