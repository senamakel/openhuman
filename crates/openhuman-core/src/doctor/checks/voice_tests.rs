use super::*;
use crate::doctor::contract::{DoctorCtx, FindingSeverity};
use crate::voice::VoiceStatus;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

struct FixedVoiceStatus {
    status: VoiceStatus,
    calls: Arc<AtomicUsize>,
    workspace: std::sync::Mutex<Option<std::path::PathBuf>>,
    launches: Arc<AtomicUsize>,
}

#[async_trait::async_trait]
impl VoiceStatusProbe for FixedVoiceStatus {
    async fn status(&self, config: &crate::config::Config) -> Result<VoiceStatus, String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        *self.workspace.lock().unwrap() = Some(config.workspace_dir.clone());
        Ok(self.status.clone())
    }
}

fn status(stt_available: bool, tts_available: bool) -> VoiceStatus {
    VoiceStatus {
        stt_available,
        tts_available,
        stt_model_id: "test-stt".into(),
        tts_voice_id: "test-voice".into(),
        piper_binary: None,
        tts_voice_path: None,
        llm_cleanup_enabled: false,
        stt_engine: "cloud".into(),
        stt_error: (!stt_available).then(|| "selected STT provider has no credential".into()),
        tts_provider: "piper".into(),
    }
}

fn config_in(root: &std::path::Path) -> crate::config::Config {
    let mut config = crate::config::Config::default();
    config.workspace_dir = root.join("workspace");
    config.action_dir = root.join("actions");
    config.config_path = root.join("config.toml");
    config
}

#[tokio::test]
async fn unavailable_stt_and_tts_are_errors_with_safe_hints() {
    let scratch = tempfile::tempdir().expect("create isolated config fixture");
    let config = config_in(scratch.path());
    let calls = Arc::new(AtomicUsize::new(0));
    let launches = Arc::new(AtomicUsize::new(0));
    let probe = Arc::new(FixedVoiceStatus {
        status: status(false, false),
        calls: Arc::clone(&calls),
        workspace: Default::default(),
        launches: Arc::clone(&launches),
    });
    let checks = checks_with_probe(probe.clone());
    let ctx = DoctorCtx::from_config(config);

    let stt = checks
        .iter()
        .find(|check| check.id() == "voice/stt")
        .expect("STT check registered")
        .detect(&ctx)
        .await
        .remove(0);
    let tts = checks
        .iter()
        .find(|check| check.id() == "voice/tts")
        .expect("TTS check registered")
        .detect(&ctx)
        .await
        .remove(0);

    assert_eq!(stt.severity, FindingSeverity::Error);
    assert_eq!(tts.severity, FindingSeverity::Error);
    assert!(!stt.message.contains("credential"));
    assert!(!tts.message.contains("test-voice"));
    assert!(!stt.fix_hint.trim().is_empty());
    assert!(!tts.fix_hint.trim().is_empty());
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(probe.launches.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn voice_checks_use_selected_b_config_and_only_inspect_paths() {
    let ambient = tempfile::tempdir().expect("create ambient config fixture");
    let selected = tempfile::tempdir().expect("create selected config fixture");
    let mut config_a = config_in(ambient.path());
    config_a.local_ai.tts_voice_id = "ambient-a-voice".into();
    let mut config_b = config_in(selected.path());
    config_b.local_ai.tts_voice_id = "selected-b-voice".into();
    let calls = Arc::new(AtomicUsize::new(0));
    let launches = Arc::new(AtomicUsize::new(0));
    let probe = Arc::new(FixedVoiceStatus {
        status: status(true, true),
        calls: Arc::clone(&calls),
        workspace: Default::default(),
        launches: Arc::clone(&launches),
    });
    let checks = checks_with_probe(probe.clone());
    let ambient_context = crate::core::runtime::CoreContext::for_test_with_config(
        crate::core::runtime::DomainSet::full(),
        config_a,
    );
    let outcome = crate::core::runtime::CoreContext::scope(ambient_context, async {
        let selected_ctx = DoctorCtx::from_config(config_b.clone()).with_components(
            crate::core::runtime::ComponentSet::from_config(&config_b.components),
        );
        let mut findings = Vec::new();
        for check in &checks {
            findings.extend(check.detect(&selected_ctx).await);
        }
        findings
    })
    .await;

    assert_eq!(outcome.len(), 2);
    assert!(
        outcome
            .iter()
            .all(|finding| finding.severity == FindingSeverity::Pass)
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(
        *probe.workspace.lock().unwrap(),
        Some(config_b.workspace_dir.clone())
    );
    assert_eq!(probe.launches.load(Ordering::SeqCst), 0);
}
