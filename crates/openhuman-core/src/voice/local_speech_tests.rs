use super::*;

#[tokio::test]
async fn synthesize_piper_rejects_empty_text() {
    let config = Config::default();
    let opts = PiperOptions::default();
    let err = synthesize_piper(&config, "", &opts).await.err().unwrap();
    assert!(err.contains("required"), "empty text must error: {err}");

    let err = synthesize_piper(&config, "   ", &opts).await.err().unwrap();
    assert!(
        err.contains("required"),
        "whitespace text must error: {err}"
    );
}

#[tokio::test]
async fn synthesize_piper_surfaces_binary_lookup_failure() {
    // Make sure a missing PIPER_BIN
    // produces an actionable error, not a panic in the spawn path.
    let _env = crate::config::test_env::EnvVarGuard::locked_unset("PIPER_BIN");

    let config = Config::default();
    let opts = PiperOptions::default();
    let result = synthesize_piper(&config, "hello world", &opts).await;

    let err = result.err().expect("missing piper must error");
    assert!(
        err.contains("piper") || err.contains("TTS"),
        "should mention piper or TTS: {err}"
    );
}

#[test]
fn synthetic_viseme_timeline_handles_whitespace_only_text() {
    // Whitespace-only input would normally be rejected upstream, but
    // the helper itself must not panic — defends against a future
    // caller that bypasses the validator.
    let frames = synthetic_viseme_timeline("   ");
    assert!(!frames.is_empty());
    // chars().filter(non-ws).count() is 0 → min 1 → 80 ms total.
    assert_eq!(frames[1].end_ms, 80);
}
