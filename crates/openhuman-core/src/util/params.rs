//! Generic JSON-RPC param deserialisation helpers.
//!
//! Every domain's `schemas.rs` decodes its controller params out of a
//! `serde_json::Map<String, Value>` by hand. [`read_required`] and
//! [`read_optional`] are the two-line core of that boilerplate — reused here
//! instead of being hand-copied per domain, so the "missing required param"
//! / "invalid '<key>': <serde error>" message shapes stay one contract.

use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

/// Deserialize a required param `key`, failing with a `String` error when
/// it's absent or doesn't match `T`'s shape.
pub fn read_required<T: DeserializeOwned>(
    params: &Map<String, Value>,
    key: &str,
) -> Result<T, String> {
    let value = params
        .get(key)
        .cloned()
        .ok_or_else(|| format!("missing required param '{key}'"))?;
    serde_json::from_value(value).map_err(|e| format!("invalid '{key}': {e}"))
}

/// Deserialize an optional param `key`. Absent or `null` yields `Ok(None)`;
/// present-but-wrong-shaped yields the same `"invalid '<key>': ..."` error as
/// [`read_required`].
pub fn read_optional<T: DeserializeOwned>(
    params: &Map<String, Value>,
    key: &str,
) -> Result<Option<T>, String> {
    match params.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => serde_json::from_value(value.clone())
            .map(Some)
            .map_err(|e| format!("invalid '{key}': {e}")),
    }
}

#[cfg(test)]
#[path = "params_tests.rs"]
mod tests;
