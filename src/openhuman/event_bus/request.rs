//! Typed request/response support for the event bus.
//!
//! This layer reuses the shared controller registry that already powers the
//! JSON-RPC transport, but exposes a richer Rust-native API with typed
//! payloads, response schemas, and separated execution logs.

use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{Map, Value};

use crate::core::{self, ControllerSchema};

/// Typed controller invocation backed by the shared registered controller
/// registry that the JSON-RPC server already uses.
#[derive(Debug, Clone)]
pub struct ControllerCall<T> {
    pub method: String,
    pub payload: T,
}

impl<T> ControllerCall<T> {
    pub fn new(method: impl Into<String>, payload: T) -> Self {
        Self {
            method: method.into(),
            payload,
        }
    }

    pub fn from_parts(
        namespace: &str,
        function: &str,
        payload: T,
    ) -> Result<Self, EventBusRequestError> {
        let method = core::all::rpc_method_from_parts(namespace, function).ok_or_else(|| {
            EventBusRequestError::UnknownMethod {
                method: format!("openhuman.{}_{}", namespace, function),
            }
        })?;
        Ok(Self { method, payload })
    }

    pub async fn execute<TRes>(self) -> Result<ControllerResponse<TRes>, EventBusRequestError>
    where
        T: Serialize,
        TRes: DeserializeOwned,
    {
        let schema = core::all::schema_for_rpc_method(&self.method).ok_or_else(|| {
            EventBusRequestError::UnknownMethod {
                method: self.method.clone(),
            }
        })?;
        let params = serialize_payload_to_params(self.payload)?;
        let redacted_params =
            crate::core::rpc_log::redact_params_for_log(&serde_json::Value::Object(params.clone()));

        tracing::debug!(
            method = %self.method,
            namespace = schema.namespace,
            function = schema.function,
            params = %redacted_params,
            "[event_bus:request] invoking registered controller"
        );

        core::all::validate_params(&schema, &params)
            .map_err(EventBusRequestError::InvalidParams)?;

        let raw = core::all::try_invoke_registered_rpc(&self.method, params)
            .await
            .ok_or_else(|| EventBusRequestError::UnregisteredHandler {
                method: self.method.clone(),
            })?
            .map_err(EventBusRequestError::HandlerFailed)?;

        let (payload, logs) = split_payload_and_logs(raw);
        let value = serde_json::from_value(payload.clone()).map_err(|source| {
            EventBusRequestError::DeserializeResponse {
                method: self.method.clone(),
                source: source.to_string(),
                payload,
            }
        })?;

        tracing::debug!(
            method = %self.method,
            log_count = logs.len(),
            "[event_bus:request] controller request completed"
        );

        Ok(ControllerResponse {
            method: self.method,
            schema,
            value,
            logs,
        })
    }
}

/// Typed controller response plus separated controller execution logs.
#[derive(Debug, Clone)]
pub struct ControllerResponse<T> {
    pub method: String,
    pub schema: ControllerSchema,
    pub value: T,
    pub logs: Vec<String>,
}

/// Errors raised by the typed event-bus request layer.
#[derive(Debug, Clone)]
pub enum EventBusRequestError {
    NotInitialized,
    UnknownMethod {
        method: String,
    },
    UnregisteredHandler {
        method: String,
    },
    SerializeRequest(String),
    InvalidParams(String),
    HandlerFailed(String),
    DeserializeResponse {
        method: String,
        source: String,
        payload: Value,
    },
}

impl std::fmt::Display for EventBusRequestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotInitialized => write!(f, "event bus has not been initialized"),
            Self::UnknownMethod { method } => {
                write!(f, "unknown registered controller method: {method}")
            }
            Self::UnregisteredHandler { method } => {
                write!(f, "registered controller method has no handler: {method}")
            }
            Self::SerializeRequest(source) => {
                write!(f, "failed to serialize request payload: {source}")
            }
            Self::InvalidParams(message) => write!(f, "{message}"),
            Self::HandlerFailed(message) => write!(f, "{message}"),
            Self::DeserializeResponse { method, source, .. } => write!(
                f,
                "failed to deserialize controller response for {method}: {source}"
            ),
        }
    }
}

impl std::error::Error for EventBusRequestError {}

fn serialize_payload_to_params<T: Serialize>(
    payload: T,
) -> Result<Map<String, Value>, EventBusRequestError> {
    match serde_json::to_value(payload)
        .map_err(|e| EventBusRequestError::SerializeRequest(e.to_string()))?
    {
        Value::Object(map) => Ok(map),
        Value::Null => Ok(Map::new()),
        other => Err(EventBusRequestError::SerializeRequest(format!(
            "expected request payload to serialize to an object or null, got {}",
            type_name(&other)
        ))),
    }
}

fn split_payload_and_logs(raw: Value) -> (Value, Vec<String>) {
    match raw {
        Value::Object(mut map) => {
            if let Some(result) = map.remove("result") {
                let logs = map
                    .remove("logs")
                    .and_then(|value| serde_json::from_value::<Vec<String>>(value).ok())
                    .unwrap_or_default();

                if map.is_empty() {
                    return (result, logs);
                }

                map.insert("result".into(), result);
                if !logs.is_empty() {
                    map.insert(
                        "logs".into(),
                        Value::Array(logs.iter().cloned().map(Value::String).collect()),
                    );
                }
                (Value::Object(map), Vec::new())
            } else {
                (Value::Object(map), Vec::new())
            }
        }
        other => (other, Vec::new()),
    }
}

fn type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_payload_and_logs_unwraps_cli_result_shape() {
        let (payload, logs) = split_payload_and_logs(serde_json::json!({
            "result": { "ok": true },
            "logs": ["one", "two"]
        }));

        assert_eq!(payload, serde_json::json!({ "ok": true }));
        assert_eq!(logs, vec!["one".to_string(), "two".to_string()]);
    }

    #[test]
    fn split_payload_and_logs_keeps_non_transport_objects_intact() {
        let value = serde_json::json!({
            "result": { "ok": true },
            "logs": ["one"],
            "extra": true
        });
        let (payload, logs) = split_payload_and_logs(value.clone());

        assert_eq!(payload, value);
        assert!(logs.is_empty());
    }
}
