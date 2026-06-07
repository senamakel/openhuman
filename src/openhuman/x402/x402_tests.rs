use base64::engine::{general_purpose::STANDARD as B64, Engine as _};
use serde_json::json;

use super::types::*;

#[test]
fn parse_payment_required_round_trips() {
    let challenge = PaymentRequired {
        x402_version: 2,
        error: Some("PAYMENT-SIGNATURE header is required".into()),
        resource: ResourceInfo {
            url: "https://api.example.com/data".into(),
            description: Some("Premium data".into()),
            mime_type: Some("application/json".into()),
        },
        accepts: vec![PaymentRequirements {
            scheme: "exact".into(),
            network: SOLANA_MAINNET_CAIP2.into(),
            amount: "10000".into(),
            asset: USDC_MINT_MAINNET.into(),
            pay_to: "2wKupLR9q6wXYppw8Gr2NvWxKBUqm4PPJKkQfoxHDBg4".into(),
            max_timeout_seconds: 60,
            extra: Some(PaymentExtra {
                fee_payer: Some("EwWqGE4ZFKLofuestmU4LDdK7XM1N4ALgdZccwYugwGd".into()),
                memo: Some("pi_3abc123".into()),
            }),
        }],
        extensions: serde_json::Map::new(),
    };
    let json_str = serde_json::to_string(&challenge).unwrap();
    let parsed: PaymentRequired = serde_json::from_str(&json_str).unwrap();
    assert_eq!(parsed.x402_version, 2);
    assert_eq!(parsed.accepts.len(), 1);
    assert_eq!(parsed.accepts[0].amount, "10000");
    assert_eq!(
        parsed.accepts[0].pay_to,
        "2wKupLR9q6wXYppw8Gr2NvWxKBUqm4PPJKkQfoxHDBg4"
    );
}

#[test]
fn solana_exact_requirement_finds_match() {
    let challenge = PaymentRequired {
        x402_version: 2,
        error: None,
        resource: ResourceInfo {
            url: "https://example.com".into(),
            description: None,
            mime_type: None,
        },
        accepts: vec![
            PaymentRequirements {
                scheme: "exact".into(),
                network: "eip155:84532".into(),
                amount: "100".into(),
                asset: "0xUsdc".into(),
                pay_to: "0xRecipient".into(),
                max_timeout_seconds: 30,
                extra: None,
            },
            PaymentRequirements {
                scheme: "exact".into(),
                network: SOLANA_MAINNET_CAIP2.into(),
                amount: "5000".into(),
                asset: USDC_MINT_MAINNET.into(),
                pay_to: "SomeRecipient".into(),
                max_timeout_seconds: 60,
                extra: None,
            },
        ],
        extensions: serde_json::Map::new(),
    };
    let sol = challenge.solana_exact_requirement().unwrap();
    assert_eq!(sol.amount, "5000");
    assert!(sol.is_solana_mainnet());
}

#[test]
fn solana_exact_requirement_returns_none_when_absent() {
    let challenge = PaymentRequired {
        x402_version: 2,
        error: None,
        resource: ResourceInfo {
            url: "https://example.com".into(),
            description: None,
            mime_type: None,
        },
        accepts: vec![PaymentRequirements {
            scheme: "exact".into(),
            network: "eip155:1".into(),
            amount: "100".into(),
            asset: "0xUsdc".into(),
            pay_to: "0xRecipient".into(),
            max_timeout_seconds: 30,
            extra: None,
        }],
        extensions: serde_json::Map::new(),
    };
    assert!(challenge.solana_exact_requirement().is_none());
}

#[test]
fn payment_extra_accessors() {
    let req = PaymentRequirements {
        scheme: "exact".into(),
        network: SOLANA_MAINNET_CAIP2.into(),
        amount: "1000".into(),
        asset: USDC_MINT_MAINNET.into(),
        pay_to: "Recipient".into(),
        max_timeout_seconds: 60,
        extra: Some(PaymentExtra {
            fee_payer: Some("FeePayer123".into()),
            memo: Some("order_456".into()),
        }),
    };
    assert_eq!(req.fee_payer_pubkey(), Some("FeePayer123"));
    assert_eq!(req.memo_value(), Some("order_456"));
}

#[test]
fn payment_extra_accessors_none() {
    let req = PaymentRequirements {
        scheme: "exact".into(),
        network: SOLANA_MAINNET_CAIP2.into(),
        amount: "1000".into(),
        asset: USDC_MINT_MAINNET.into(),
        pay_to: "Recipient".into(),
        max_timeout_seconds: 60,
        extra: None,
    };
    assert_eq!(req.fee_payer_pubkey(), None);
    assert_eq!(req.memo_value(), None);
}

#[test]
fn settlement_response_deserializes_success() {
    let json_str = r#"{
        "success": true,
        "transaction": "4vJ9YFuPzUgdLkWYJf3Kqf",
        "network": "solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp",
        "payer": "EwWqGE4ZFKLofuestmU4LDdK7XM1N4ALgdZccwYugwGd"
    }"#;
    let resp: SettlementResponse = serde_json::from_str(json_str).unwrap();
    assert!(resp.success);
    assert_eq!(resp.transaction, "4vJ9YFuPzUgdLkWYJf3Kqf");
    assert!(resp.error_reason.is_none());
}

#[test]
fn settlement_response_deserializes_failure() {
    let json_str = r#"{
        "success": false,
        "transaction": "",
        "network": "solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp",
        "payer": "EwWqGE4ZFKLofuestmU4LDdK7XM1N4ALgdZccwYugwGd",
        "errorReason": "insufficient_funds"
    }"#;
    let resp: SettlementResponse = serde_json::from_str(json_str).unwrap();
    assert!(!resp.success);
    assert_eq!(resp.error_reason.as_deref(), Some("insufficient_funds"));
}

#[test]
fn base64_header_round_trip() {
    let challenge = PaymentRequired {
        x402_version: 2,
        error: None,
        resource: ResourceInfo {
            url: "https://example.com/api".into(),
            description: None,
            mime_type: None,
        },
        accepts: vec![PaymentRequirements {
            scheme: "exact".into(),
            network: SOLANA_MAINNET_CAIP2.into(),
            amount: "1000000".into(),
            asset: USDC_MINT_MAINNET.into(),
            pay_to: "RecipientPubkey".into(),
            max_timeout_seconds: 60,
            extra: None,
        }],
        extensions: serde_json::Map::new(),
    };
    let json_bytes = serde_json::to_vec(&challenge).unwrap();
    let encoded = B64.encode(&json_bytes);
    let decoded = B64.decode(&encoded).unwrap();
    let parsed: PaymentRequired = serde_json::from_slice(&decoded).unwrap();
    assert_eq!(parsed.accepts[0].amount, "1000000");
}
