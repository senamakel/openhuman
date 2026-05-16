use serde::de::DeserializeOwned;
use serde_json::{json, Value};

use super::defaults::rpc_url_for_chain;
use super::ops::WalletChain;

const LOG_PREFIX: &str = "[wallet::rpc]";

pub async fn rpc_call<T: DeserializeOwned>(
    chain: WalletChain,
    method: &str,
    params: Value,
) -> Result<T, String> {
    let url = rpc_url_for_chain(chain);
    let payload = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": method,
        "params": params,
    });
    log::debug!("{LOG_PREFIX} chain={:?} method={} url={}", chain, method, url);
    let response = reqwest::Client::new()
        .post(url)
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("wallet RPC transport failed for {method}: {e}"))?;
    let status = response.status();
    let body: Value = response
        .json()
        .await
        .map_err(|e| format!("wallet RPC decode failed for {method}: {e}"))?;
    if !status.is_success() {
        return Err(format!("wallet RPC HTTP failure for {method}: status={status} body={body}"));
    }
    if let Some(error) = body.get("error") {
        return Err(format!("wallet RPC error for {method}: {error}"));
    }
    let result = body
        .get("result")
        .cloned()
        .ok_or_else(|| format!("wallet RPC missing result for {method}"))?;
    serde_json::from_value(result).map_err(|e| format!("wallet RPC invalid result for {method}: {e}"))
}
