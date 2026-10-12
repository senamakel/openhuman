use super::*;
#[tokio::test]
async fn cloud_transcription_dispatches_through_the_installed_host_provider() {
    use crate::platform::cloud_voice_provider::{self, CloudVoiceProviderFactory};
    use std::sync::Arc;
    use tinyvoice_bus::provider::{
        CloudVoiceProvider, ProviderFuture, RealtimeRequest, RealtimeSession, SynthesisRequest,
        Transcription, TranscriptionRequest,
    };

    struct Provider;
    impl CloudVoiceProvider for Provider {
        fn transcribe(&self, request: TranscriptionRequest) -> ProviderFuture<'_, Transcription> {
            Box::pin(async move {
                assert_eq!(request.audio_base64, "AQID");
                assert_eq!(request.language.as_deref(), Some("fr"));
                Ok(Transcription {
                    text: "host provider transcript".into(),
                })
            })
        }
        fn synthesize(&self, _: SynthesisRequest) -> ProviderFuture<'_, serde_json::Value> {
            Box::pin(std::future::ready(Err(
                tinyvoice_bus::Error::InvalidInput {
                    message: "not requested".into(),
                },
            )))
        }
        fn realtime(&self, _: RealtimeRequest) -> ProviderFuture<'_, RealtimeSession> {
            Box::pin(std::future::ready(Err(
                tinyvoice_bus::Error::InvalidInput {
                    message: "not requested".into(),
                },
            )))
        }
    }
    struct Factory;
    impl CloudVoiceProviderFactory for Factory {
        fn for_config(&self, _: &Config) -> Result<Arc<dyn CloudVoiceProvider>, String> {
            Ok(Arc::new(Provider))
        }
    }
    struct Restore(Option<Arc<dyn CloudVoiceProviderFactory>>);
    impl Drop for Restore {
        fn drop(&mut self) {
            cloud_voice_provider::restore(self.0.take());
        }
    }
    let _restore = Restore(cloud_voice_provider::install(Arc::new(Factory)));
    let temp = tempfile::TempDir::new().unwrap();
    let config = Config {
        workspace_dir: temp.path().join("workspace"),
        action_dir: temp.path().join("action"),
        config_path: temp.path().join("config.toml"),
        ..Config::default()
    };
    let outcome = transcribe_cloud(
        &config,
        "AQID",
        &CloudTranscribeOptions {
            language: Some("fr".into()),
            ..CloudTranscribeOptions::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(outcome.value.text, "host provider transcript");
}
