//! Tests for the ML sidecar that don't require torch.
//!
//! - protocol serialization round-trips (covered in `protocol.rs` too)
//! - a tiny mock worker (plain `python3` echo script) exercises the handshake,
//!   id-matching, and compress round-trip without any ML deps. Skipped when no
//!   system `python3` is on PATH.
//! - a gated real integration test behind `OPENHUMAN_TEST_ML_KOMPRESS`.

use super::protocol::{ReadyLine, WorkerRequest, WorkerResponse};
use super::provision::KompressRuntime;
use super::sidecar::{MlSidecarOpts, PythonCompressorSidecar};

const MOCK_WORKER: &str = r#"
import json, sys
sys.stdout.write(json.dumps({"ready": True, "model": "mock", "device": "cpu"}) + "\n")
sys.stdout.flush()
for line in sys.stdin:
    line = line.strip()
    if not line:
        continue
    req = json.loads(line)
    op = req.get("op", "compress")
    rid = req.get("id")
    if op == "ping":
        sys.stdout.write(json.dumps({"id": rid, "pong": True}) + "\n")
    elif op == "shutdown":
        sys.stdout.write(json.dumps({"id": rid, "pong": True}) + "\n")
        sys.stdout.flush()
        break
    else:
        text = req.get("text", "")
        out = text[: max(1, len(text) // 2)]
        sys.stdout.write(json.dumps({"id": rid, "compressed_text": out,
            "stats": {"input_chars": len(text), "output_chars": len(out),
                      "ratio": 0.5, "model_ms": 1}}) + "\n")
    sys.stdout.flush()
"#;

fn system_python() -> Option<String> {
    for cand in ["python3", "python"] {
        if std::process::Command::new(cand)
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
        {
            return Some(cand.to_string());
        }
    }
    None
}

#[test]
fn protocol_request_serializes() {
    let req = WorkerRequest::Ping { id: "9".into() };
    let s = serde_json::to_string(&req).unwrap();
    assert!(s.contains("\"op\":\"ping\""), "{s}");
}

#[test]
fn ready_and_response_parse() {
    let r: ReadyLine = serde_json::from_str(r#"{"ready":true,"model":"m"}"#).unwrap();
    assert!(r.ready);
    let resp: WorkerResponse =
        serde_json::from_str(r#"{"id":"1","compressed_text":"x"}"#).unwrap();
    assert_eq!(resp.compressed_text.as_deref(), Some("x"));
}

#[tokio::test]
async fn mock_worker_handshake_and_compress() {
    let Some(py) = system_python() else {
        eprintln!("no system python; skipping mock worker test");
        return;
    };
    // Write the mock worker to a temp script.
    let dir = std::env::temp_dir().join("tj-ml-mock");
    std::fs::create_dir_all(&dir).unwrap();
    let script = dir.join("mock_worker.py");
    std::fs::write(&script, MOCK_WORKER).unwrap();

    let runtime = KompressRuntime {
        python_bin: std::path::PathBuf::from(py),
        worker_script: script,
        hf_home: dir.clone(),
    };
    let opts = MlSidecarOpts {
        model_id: "mock".into(),
        device: "cpu".into(),
        target_ratio: 0.5,
        max_input_chars: 100_000,
    };
    let sidecar = PythonCompressorSidecar::spawn(&runtime, &opts)
        .await
        .expect("mock worker spawns + handshakes");
    let input = "this is a reasonably long sentence to be halved by the mock worker";
    let res = sidecar
        .compress(input, &runtime, &opts)
        .await
        .expect("compress round-trip");
    assert!(res.text.len() <= input.len());
    assert!(!res.text.is_empty());

    let _ = std::fs::remove_dir_all(&dir);
}

/// Real torch + ModernBERT integration. Gated behind an env flag and skipped in
/// default CI (no torch / no network). Set OPENHUMAN_TEST_ML_KOMPRESS=1 and
/// enable runtime_python + tokenjuice.ml_compression_enabled to run.
#[tokio::test]
#[ignore = "requires torch + model download; opt in via OPENHUMAN_TEST_ML_KOMPRESS"]
async fn real_kompress_compresses() {
    if std::env::var("OPENHUMAN_TEST_ML_KOMPRESS").is_err() {
        return;
    }
    let mut config = crate::openhuman::config::Config::default();
    config.runtime_python.enabled = true;
    config.tokenjuice.ml_compression_enabled = true;
    super::configure(config);

    let text = "The quick brown fox. ".repeat(40);
    let opts = crate::openhuman::tokenjuice::types::CompressOptions::default();
    match super::compress(&text, &opts).await {
        Ok(Some(out)) => assert!(out.len() < text.len()),
        other => panic!("expected compression, got {other:?}"),
    }
}
