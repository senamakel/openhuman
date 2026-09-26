//! Billing and payment RPC ops — thin adapters that call the hosted API.
//!
//! # Security
//! All methods require a valid app-session JWT stored via `auth_store_session`.
//! The JWT is sent as `Authorization: Bearer …` to the backend.
//! **No server-side authorization is replicated here**: the backend enforces plan
//! ownership, tenant isolation, and payment policy on every request.
//! Callers that lack a valid session or sufficient permissions receive a
//! backend 401/403 error surfaced verbatim as an RPC error string.
//! API keys / JWTs are never written to logs (only redacted status codes + paths).

use reqwest::Method;
use serde_json::{Map, Value};
use tinyhumans_sdk::api::payments::{
    CoinbaseInterval, CoinbasePlan, CreateCoinbaseChargeRequest,
    CreditTopUpRequest, PaymentGateway, PurchaseStripePlanRequest,
    UpdateAutoRechargeCardRequest,
};
use tinyhumans_sdk::api::types::{BillingPlan, CodeRequest};

use openhuman_core::config::Config;
use openhuman_core::rpc::RpcOutcome;

use crate::hosted::client::HostedClient;

/// Parse a wire string into one of the SDK's closed request enums, naming the
/// field and the offending value on failure.
fn parse_enum<T: serde::de::DeserializeOwned>(field: &str, raw: &str) -> Result<T, String> {
    serde_json::from_value(Value::String(raw.to_string()))
        .map_err(|_| format!("unsupported {field}: {raw}"))
}

pub async fn get_current_plan(config: &Config) -> Result<RpcOutcome<Value>, String> {
    let client = HostedClient::from_config(config)?;
    let data = client.finish_value(
        "GET /payments/stripe/currentPlan",
        client.sdk().payments().get_current_plan().await,
    )?;
    Ok(RpcOutcome::single_log(
        data,
        "current plan fetched from backend",
    ))
}

pub async fn get_summary(config: &Config) -> Result<RpcOutcome<Value>, String> {
    let client = HostedClient::from_config(config)?;
    let data = client.finish_value(
        "GET /payments/summary",
        client.sdk().payments().get_summary().await,
    )?;
    Ok(RpcOutcome::single_log(data, "billing summary fetched"))
}

pub async fn get_balance(config: &Config) -> Result<RpcOutcome<Value>, String> {
    let client = HostedClient::from_config(config)?;
    let data = client.finish_value(
        "GET /payments/credits/balance",
        client.sdk().payments().get_credit_balance().await,
    )?;
    Ok(RpcOutcome::single_log(data, "credit balance fetched"))
}

pub async fn get_transactions(
    config: &Config,
    limit: Option<u64>,
    offset: Option<u64>,
) -> Result<RpcOutcome<Value>, String> {
    let limit = limit.unwrap_or(20);
    let offset = offset.unwrap_or(0);
    let client = HostedClient::from_config(config)?;
    let query = [
        ("limit", Some(limit.to_string())),
        ("offset", Some(offset.to_string())),
    ];
    let data = client.finish_value(
        "GET /payments/credits/transactions",
        client
            .sdk()
            .payments()
            .list_credit_transactions(&query)
            .await,
    )?;
    Ok(RpcOutcome::single_log(data, "credit transactions fetched"))
}

pub async fn get_auto_recharge(config: &Config) -> Result<RpcOutcome<Value>, String> {
    let client = HostedClient::from_config(config)?;
    let data = client.finish_value(
        "GET /payments/credits/auto-recharge",
        client.sdk().payments().get_auto_recharge().await,
    )?;
    Ok(RpcOutcome::single_log(
        data,
        "auto recharge settings fetched",
    ))
}

/// `PATCH /payments/credits/auto-recharge`.
///
/// Forwarded verbatim on the SDK's raw primitive (route policy still applies)
/// rather than through the typed `update_auto_recharge`: the SDK's
/// `AutoRechargeRequest` has no `weeklyLimitUsd`, which the settings UI sends,
/// so decoding into it would silently drop the weekly cap.
pub async fn update_auto_recharge(
    config: &Config,
    payload: Value,
) -> Result<RpcOutcome<Value>, String> {
    let client = HostedClient::from_config(config)?;
    let data = client.finish(
        "PATCH /payments/credits/auto-recharge",
        client
            .sdk()
            .raw()
            .send(
                Method::PATCH,
                "/payments/credits/auto-recharge",
                &[],
                Some(&payload),
                true,
            )
            .await,
    )?;
    Ok(RpcOutcome::single_log(
        data,
        "auto recharge settings updated",
    ))
}

pub async fn get_cards(config: &Config) -> Result<RpcOutcome<Value>, String> {
    let client = HostedClient::from_config(config)?;
    let data = client.finish_value(
        "GET /payments/credits/auto-recharge/cards",
        client.sdk().payments().list_auto_recharge_cards().await,
    )?;
    Ok(RpcOutcome::single_log(data, "saved cards fetched"))
}

pub async fn create_setup_intent(config: &Config) -> Result<RpcOutcome<Value>, String> {
    let client = HostedClient::from_config(config)?;
    let data = client.finish_value(
        "POST /payments/credits/auto-recharge/cards/setup-intent",
        client
            .sdk()
            .payments()
            .create_auto_recharge_card_setup_intent()
            .await,
    )?;
    Ok(RpcOutcome::single_log(data, "setup intent created"))
}

pub async fn update_card(
    config: &Config,
    payment_method_id: &str,
    payload: Value,
) -> Result<RpcOutcome<Value>, String> {
    let payment_method_id = payment_method_id.trim();
    if payment_method_id.is_empty() {
        return Err("paymentMethodId is required".to_string());
    }
    let fields = match payload {
        Value::Object(map) => map,
        Value::Null => Map::new(),
        _ => return Err("card update payload must be an object".to_string()),
    };
    let client = HostedClient::from_config(config)?;
    let data = client.finish_value(
        "PATCH /payments/credits/auto-recharge/cards/{paymentMethodId}",
        client
            .sdk()
            .payments()
            .update_auto_recharge_card(payment_method_id, &UpdateAutoRechargeCardRequest { fields })
            .await,
    )?;
    Ok(RpcOutcome::single_log(data, "saved card updated"))
}

pub async fn delete_card(
    config: &Config,
    payment_method_id: &str,
) -> Result<RpcOutcome<Value>, String> {
    let payment_method_id = payment_method_id.trim();
    if payment_method_id.is_empty() {
        return Err("paymentMethodId is required".to_string());
    }
    let client = HostedClient::from_config(config)?;
    let data = client.finish_value(
        "DELETE /payments/credits/auto-recharge/cards/{paymentMethodId}",
        client
            .sdk()
            .payments()
            .delete_auto_recharge_card(payment_method_id)
            .await,
    )?;
    Ok(RpcOutcome::single_log(data, "saved card deleted"))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PurchasePlanBody<'a> {
    plan: &'a str,
}

pub async fn purchase_plan(config: &Config, plan: &str) -> Result<RpcOutcome<Value>, String> {
    let plan = plan.trim();
    if plan.is_empty() {
        return Err("plan is required".to_string());
    }

    let body = json!(PurchasePlanBody { plan });
    let data = get_authed_value(
        config,
        Method::POST,
        "/payments/stripe/purchasePlan",
        Some(body),
    )
    .await?;

    Ok(RpcOutcome::single_log(
        data,
        "plan purchase session created",
    ))
}

pub async fn create_portal_session(config: &Config) -> Result<RpcOutcome<Value>, String> {
    let data = get_authed_value(config, Method::POST, "/payments/stripe/portal", None).await?;
    Ok(RpcOutcome::single_log(
        data,
        "customer portal session created",
    ))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TopUpBody {
    amount_usd: f64,
    #[serde(default = "default_gateway")]
    gateway: String,
}

fn default_gateway() -> String {
    "stripe".to_string()
}

fn normalize_gateway(gateway: Option<String>) -> Result<String, String> {
    let gateway = gateway
        .as_deref()
        .map(str::trim)
        .filter(|g| !g.is_empty())
        .map(str::to_ascii_lowercase)
        .unwrap_or_else(default_gateway);

    if !matches!(gateway.as_str(), "stripe" | "coinbase") {
        return Err("gateway must be one of: stripe, coinbase".to_string());
    }

    Ok(gateway)
}

pub async fn top_up_credits(
    config: &Config,
    amount_usd: f64,
    gateway: Option<String>,
) -> Result<RpcOutcome<Value>, String> {
    if !amount_usd.is_finite() || amount_usd <= 0.0 {
        return Err("amountUsd must be a finite number greater than 0".to_string());
    }

    let gateway = normalize_gateway(gateway)?;
    let body = TopUpBody {
        amount_usd,
        gateway,
    };

    let data = get_authed_value(
        config,
        Method::POST,
        "/payments/credits/top-up",
        Some(json!(body)),
    )
    .await?;

    Ok(RpcOutcome::single_log(data, "credit top-up initiated"))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CoinbaseChargeBody<'a> {
    plan: &'a str,
    interval: &'a str,
}

/// Create a Coinbase Commerce charge (the "payment link" for crypto / annual billing).
/// Maps to `POST /payments/coinbase/charge` — matches `billingApi.createCoinbaseCharge`.
pub async fn create_coinbase_charge(
    config: &Config,
    plan: &str,
    interval: Option<String>,
) -> Result<RpcOutcome<Value>, String> {
    let plan = plan.trim();
    if plan.is_empty() {
        return Err("plan is required".to_string());
    }

    let interval_str = interval
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("annual");

    let body = json!(CoinbaseChargeBody {
        plan,
        interval: interval_str,
    });

    let data = get_authed_value(
        config,
        Method::POST,
        "/payments/coinbase/charge",
        Some(body),
    )
    .await?;

    Ok(RpcOutcome::single_log(
        data,
        "Coinbase payment link created",
    ))
}

// ── Coupon operations ──────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
struct RedeemCouponBody<'a> {
    code: &'a str,
}

/// Redeem a coupon code to add credits to the user's account.
/// Maps to `POST /coupons/redeem`.
pub async fn redeem_coupon(config: &Config, code: &str) -> Result<RpcOutcome<Value>, String> {
    let code = code.trim();
    if code.is_empty() {
        return Err("code is required".to_string());
    }

    let body = json!(RedeemCouponBody { code });
    let data = get_authed_value(config, Method::POST, "/coupons/redeem", Some(body)).await?;

    Ok(RpcOutcome::single_log(data, "coupon redeemed"))
}

/// List coupons redeemed by the current user.
/// Maps to `GET /coupons/me`.
pub async fn get_user_coupons(config: &Config) -> Result<RpcOutcome<Value>, String> {
    let data = get_authed_value(config, Method::GET, "/coupons/me", None).await?;
    Ok(RpcOutcome::single_log(data, "user coupons fetched"))
}

#[cfg(test)]
#[path = "ops_tests.rs"]
mod tests;
