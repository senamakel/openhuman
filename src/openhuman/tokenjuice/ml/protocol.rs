//! Wire protocol for the TokenJuice ML ("Kompress") sidecar.
//!
//! Line-delimited JSON over the worker's stdin/stdout. These types are
//! torch-free so the protocol can be unit-tested without any Python/ML deps.
//!
//! Handshake (worker → Rust), one or more lines until ready:
//! ```text
//! {"status":"downloading","model":"answerdotai/ModernBERT-base"}
//! {"status":"loading","model":"answerdotai/ModernBERT-base"}
//! {"ready":true,"model":"answerdotai/ModernBERT-base","device":"cpu"}
//! ```
//! Request (Rust → worker): `{"id":"1","op":"compress","text":"…","target_ratio":0.5,"max_input_chars":200000}`
//! Response (worker → Rust): `{"id":"1","compressed_text":"…","stats":{…}}` or `{"id":"1","error":"…"}`

use serde::{Deserialize, Serialize};

/// A request sent to the worker.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum WorkerRequest<'a> {
    Compress {
        id: String,
        text: &'a str,
        target_ratio: f64,
        max_input_chars: usize,
    },
    Ping {
        id: String,
    },
    Shutdown {
        id: String,
    },
}

/// A handshake line emitted by the worker before it is ready.
#[derive(Debug, Clone, Deserialize)]
pub struct ReadyLine {
    #[serde(default)]
    pub ready: bool,
    /// Progress status before `ready` (`downloading` / `loading`).
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub device: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
}

impl ReadyLine {
    /// True if this is an intermediate progress line (keepalive, keep waiting).
    pub fn is_progress(&self) -> bool {
        !self.ready && self.error.is_none() && self.status.is_some()
    }
}

/// Stats reported alongside a compression response.
#[derive(Debug, Clone, Deserialize)]
pub struct CompressStats {
    #[serde(default)]
    pub input_chars: usize,
    #[serde(default)]
    pub output_chars: usize,
    #[serde(default)]
    pub ratio: f64,
    #[serde(default)]
    pub model_ms: u64,
}

/// A response to a compress/ping request.
#[derive(Debug, Clone, Deserialize)]
pub struct WorkerResponse {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub compressed_text: Option<String>,
    #[serde(default)]
    pub stats: Option<CompressStats>,
    /// Set on a `ping` reply.
    #[serde(default)]
    pub pong: bool,
    #[serde(default)]
    pub error: Option<String>,
}

/// Validated result of one compression round-trip.
#[derive(Debug, Clone)]
pub struct MlCompressResult {
    pub text: String,
    pub stats: Option<CompressStats>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_compress_request() {
        let req = WorkerRequest::Compress {
            id: "7".into(),
            text: "hello world",
            target_ratio: 0.5,
            max_input_chars: 1000,
        };
        let s = serde_json::to_string(&req).unwrap();
        assert!(s.contains("\"op\":\"compress\""), "{s}");
        assert!(s.contains("\"id\":\"7\""));
        assert!(s.contains("\"target_ratio\":0.5"));
    }

    #[test]
    fn parses_progress_then_ready() {
        let prog: ReadyLine =
            serde_json::from_str(r#"{"status":"downloading","model":"m"}"#).unwrap();
        assert!(prog.is_progress());
        assert!(!prog.ready);

        let ready: ReadyLine =
            serde_json::from_str(r#"{"ready":true,"model":"m","device":"cpu"}"#).unwrap();
        assert!(ready.ready);
        assert!(!ready.is_progress());
        assert_eq!(ready.device.as_deref(), Some("cpu"));
    }

    #[test]
    fn parses_response_and_error() {
        let ok: WorkerResponse = serde_json::from_str(
            r#"{"id":"1","compressed_text":"short","stats":{"input_chars":100,"output_chars":50,"ratio":0.5,"model_ms":12}}"#,
        )
        .unwrap();
        assert_eq!(ok.id.as_deref(), Some("1"));
        assert_eq!(ok.compressed_text.as_deref(), Some("short"));
        assert_eq!(ok.stats.unwrap().ratio, 0.5);

        let err: WorkerResponse = serde_json::from_str(r#"{"id":"2","error":"boom"}"#).unwrap();
        assert_eq!(err.error.as_deref(), Some("boom"));
    }
}
