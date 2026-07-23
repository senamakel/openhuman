//! Wire-level frame envelopes for the `medulla-serve` NDJSON protocol.
//!
//! The contract follows the medulla-serve NDJSON protocol, v1 (plan §2.2).
//! `serve` is the Node child wrapping medulla-v1's agent-harness facade;
//! `host` is this Rust supervisor. Six frame kinds cross three flows:
//!
//! | `t`     | direction   | flow                                    |
//! | ------- | ----------- | --------------------------------------- |
//! | `ready` | serve→host  | handshake banner (unprompted, first)    |
//! | `req`   | host→serve  | request                                 |
//! | `res`   | serve→host  | response (correlated by `id`)           |
//! | `call`  | serve→host  | port callback (reverse RPC into host)   |
//! | `ret`   | host→serve  | port-callback return (correlated `id`)  |
//! | `emit`  | host→serve  | streaming tap for an in-flight `call`   |
//! | `event` | serve→host  | unsolicited event stream (no `id`)      |
//!
//! Frames are read as untyped [`serde_json::Value`] and dispatched on the `t`
//! discriminant (see [`FrameKind`]) so an unknown/extra frame is skipped and
//! logged rather than treated as fatal — mirroring
//! `runtime_python_server/server.rs`'s "unparseable response skipped".

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// Wire version the host speaks. The `ready` banner MUST carry the same value
/// or the host bails (§3, §7 handshake mismatch).
pub const PROTOCOL_VERSION: u32 = 1;

/// The `t` discriminant on an inbound (serve→host) frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameKind {
    Ready,
    Res,
    Call,
    Event,
    /// Any `t` value the host does not expect inbound (`req`/`ret`/`emit` are
    /// host→serve). Skipped and logged.
    Unknown,
}

impl FrameKind {
    /// Classify a decoded frame by its `t` field.
    pub fn of(frame: &Value) -> Self {
        match frame.get("t").and_then(Value::as_str) {
            Some("ready") => Self::Ready,
            Some("res") => Self::Res,
            Some("call") => Self::Call,
            Some("event") => Self::Event,
            _ => Self::Unknown,
        }
    }
}

/// The unprompted first line serve writes on connect (§3). `error` non-null ⇒
/// startup failed and the host treats the child as unavailable.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadyLine {
    #[serde(default)]
    pub protocol: u32,
    #[serde(default)]
    pub serve: Option<String>,
    #[serde(default, rename = "sessionId")]
    pub session_id: Option<String>,
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub error: Option<String>,
}

/// The always-`{code,message}` error envelope shared by `res` and `ret`
/// (§8). Mirrors the `PythonServerError` shape.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServeError {
    pub code: String,
    pub message: String,
}

impl ServeError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

/// A decoded `res` frame (serve→host response, correlated by `id`).
#[derive(Debug, Clone, Deserialize)]
pub struct ResFrame {
    pub id: Option<String>,
    #[serde(default)]
    pub ok: bool,
    #[serde(default)]
    pub result: Option<Value>,
    #[serde(default)]
    pub error: Option<ServeError>,
}

/// A decoded `call` frame (serve→host reverse-RPC into a host port).
#[derive(Debug, Clone, Deserialize)]
pub struct CallFrame {
    pub id: String,
    pub port: String,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

/// A decoded `event` frame (serve→host unsolicited stream, §6).
#[derive(Debug, Clone, Deserialize)]
pub struct EventFrame {
    #[serde(default)]
    pub seq: u64,
    #[serde(default)]
    pub at: u64,
    #[serde(default)]
    pub event: Value,
}

/// Build a host→serve `req` frame line body (§4).
pub fn req_frame(id: &str, op: &str, params: Value) -> Value {
    json!({ "t": "req", "id": id, "op": op, "params": params })
}

/// Build a host→serve `ret` frame answering a port `call` with success (§5).
pub fn ret_ok(id: &str, result: Value) -> Value {
    json!({ "t": "ret", "id": id, "ok": true, "result": result })
}

/// Build a host→serve `ret` frame answering a port `call` with failure (§5, §8).
pub fn ret_err(id: &str, error: &ServeError) -> Value {
    json!({ "t": "ret", "id": id, "ok": false, "error": { "code": error.code, "message": error.message } })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ready_line_parses_session_and_capabilities() {
        let ready: ReadyLine = serde_json::from_str(
            r#"{"t":"ready","protocol":1,"serve":"3.12.0","sessionId":"agent","capabilities":["inference","tools"],"error":null}"#,
        )
        .unwrap();
        assert_eq!(ready.protocol, PROTOCOL_VERSION);
        assert_eq!(ready.session_id.as_deref(), Some("agent"));
        assert_eq!(ready.capabilities, vec!["inference", "tools"]);
        assert!(ready.error.is_none());
    }

    #[test]
    fn frame_kind_classifies_by_t() {
        assert_eq!(
            FrameKind::of(&json!({"t":"call","id":"c1"})),
            FrameKind::Call
        );
        assert_eq!(FrameKind::of(&json!({"t":"event"})), FrameKind::Event);
        assert_eq!(FrameKind::of(&json!({"t":"nope"})), FrameKind::Unknown);
        assert_eq!(FrameKind::of(&json!({})), FrameKind::Unknown);
    }

    #[test]
    fn res_error_envelope_parses() {
        let res: ResFrame = serde_json::from_str(
            r#"{"t":"res","id":"7","ok":false,"error":{"code":"bad_request","message":"missing message"}}"#,
        )
        .unwrap();
        assert!(!res.ok);
        assert_eq!(res.id.as_deref(), Some("7"));
        assert_eq!(res.error.unwrap().code, "bad_request");
    }

    #[test]
    fn ret_frames_carry_correct_discriminant() {
        let ok = ret_ok("c1", json!({"content": []}));
        assert_eq!(ok["t"], "ret");
        assert_eq!(ok["ok"], true);
        let err = ret_err("c2", &ServeError::new("port_unavailable", "no memory port"));
        assert_eq!(err["t"], "ret");
        assert_eq!(err["ok"], false);
        assert_eq!(err["error"]["code"], "port_unavailable");
    }
}
