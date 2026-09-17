//! `tinyjevclient` implementation of TinyHiveMind's System One transport port.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde::Serialize;
use serde_json::Value;
use tinyhivemind_typesafe::{
    SystemOneRequest, SystemOneTransport, SystemOneTransportFuture, TransportError,
};
use tinyjevclient::{Client, EvaluationRequest};

/// One complete provider exchange, without credentials.
#[derive(Clone, Debug, Serialize)]
pub(crate) struct SystemOneTrace {
    /// Invocation sequence, assigned before the provider wait.
    pub(crate) sequence: u64,
    /// Exact structured request sent to System One.
    pub(crate) request: Value,
    /// Exact typed response returned by System One.
    pub(crate) response: Value,
    /// Native client attempts, including retries.
    pub(crate) attempts: u32,
    /// End-to-end provider latency.
    pub(crate) latency_ms: u64,
}

/// Native Jev client behind the reusable TinyHiveMind transport port.
#[derive(Clone)]
pub(crate) struct JevTransport {
    client: Client,
    traces: Arc<Mutex<Vec<SystemOneTrace>>>,
    next_sequence: Arc<AtomicU64>,
}

impl JevTransport {
    /// Build from `TYPESAFE_API_KEY` and production client defaults.
    pub(crate) fn from_env() -> Result<Self, tinyjevclient::Error> {
        Ok(Self {
            client: Client::from_env()?,
            traces: Arc::new(Mutex::new(Vec::new())),
            next_sequence: Arc::new(AtomicU64::new(0)),
        })
    }

    /// Snapshot completed calls in request order.
    pub(crate) fn traces(&self) -> Result<Vec<SystemOneTrace>, String> {
        let mut traces = self
            .traces
            .lock()
            .map(|traces| traces.clone())
            .map_err(|_| "System One trace lock was poisoned".to_owned())?;
        sort_traces(&mut traces);
        Ok(traces)
    }
}

/// Restore invocation order after provider calls complete out of order.
fn sort_traces(traces: &mut [SystemOneTrace]) {
    traces.sort_by_key(|trace| trace.sequence);
}

impl SystemOneTransport for JevTransport {
    fn evaluate<'a>(&'a self, request: &'a SystemOneRequest) -> SystemOneTransportFuture<'a> {
        Box::pin(async move {
            let sequence = self.next_sequence.fetch_add(1, Ordering::Relaxed);
            let (wire_request, native_request) = native_request(request)?;
            let started = Instant::now();
            let result = self
                .client
                .evaluate(&native_request)
                .await
                .map_err(transport_error)?;
            let wire_response = serde_json::to_value(&result.response).map_err(transport_error)?;
            let latency_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
            self.traces
                .lock()
                .map_err(|_| TransportError::Transport {
                    status: None,
                    message: "System One trace lock was poisoned".to_owned(),
                })?
                .push(SystemOneTrace {
                    sequence,
                    request: wire_request,
                    response: wire_response.clone(),
                    attempts: result.attempts,
                    latency_ms,
                });
            let response = serde_json::from_value(wire_response).map_err(transport_error)?;
            Ok(response)
        })
    }
}

fn native_request(
    request: &SystemOneRequest,
) -> Result<(Value, EvaluationRequest), TransportError> {
    let wire = serde_json::to_value(request).map_err(transport_error)?;
    let native = serde_json::from_value(wire.clone()).map_err(transport_error)?;
    Ok((wire, native))
}

/// Convert any native-client or wire failure into the transport port's error.
fn transport_error(error: impl std::fmt::Display) -> TransportError {
    TransportError::Transport {
        status: None,
        message: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::json;
    use tinyhivemind_typesafe::{Question, SystemOneRequest, SystemOneResponse};
    use tinyjevclient::{Answer, EvaluationResponse, NoulAnswer, Usage};

    use super::{native_request, sort_traces, transport_error, SystemOneTrace};

    #[test]
    fn adapter_errors_do_not_invent_an_http_status() {
        let error = transport_error("bad wire");
        let tinyhivemind_typesafe::Error::Transport { status, message } = error else {
            panic!("adapter errors must use the transport variant");
        };
        assert_eq!(status, None);
        assert_eq!(message, "bad wire");
    }

    #[test]
    fn adapter_preserves_the_exact_request_and_response_wires() {
        let request = SystemOneRequest {
            state: json!({"message":"route me"}),
            model: "jev-latest".into(),
            questions: BTreeMap::from([(
                "high_impact".into(),
                Question::Noul {
                    instructions: json!("Is this high impact?"),
                    criteria: None,
                },
            )]),
        };
        let (wire, native) = native_request(&request).expect("request converts");
        assert_eq!(
            wire,
            serde_json::to_value(&native).expect("native serializes")
        );

        let native_response = EvaluationResponse {
            model: "jev-test".into(),
            answers: BTreeMap::from([(
                "high_impact".into(),
                Answer::Noul(NoulAnswer { noul: 0.75 }),
            )]),
            usage: Usage {
                input_tokens: Some(12),
                output_tokens: Some(3),
            },
        };
        let converted: SystemOneResponse = serde_json::from_value(
            serde_json::to_value(native_response).expect("response serializes"),
        )
        .expect("response converts");
        assert_eq!(converted.model, "jev-test");
        assert_eq!(converted.usage.input_tokens, 12);
        assert_eq!(converted.usage.output_tokens, 3);
    }

    #[test]
    fn traces_are_reported_in_invocation_order() {
        let mut traces = [
            SystemOneTrace {
                sequence: 2,
                request: json!({}),
                response: json!({}),
                attempts: 1,
                latency_ms: 1,
            },
            SystemOneTrace {
                sequence: 1,
                request: json!({}),
                response: json!({}),
                attempts: 1,
                latency_ms: 2,
            },
        ];
        sort_traces(&mut traces);
        assert_eq!(traces.map(|trace| trace.sequence), [1, 2]);
    }
}
