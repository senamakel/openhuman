//! Turn observers never receive payloads or raw errors by default.
mod common;
use openhuman_embed::observe::{
    observe_turn, TraceContent, TurnObservation, TurnObserver, TurnTrace,
};
use openhuman_embed::CoreError;
use std::sync::{Arc, Mutex};
#[derive(Default)]
struct Recorder(Mutex<Vec<String>>);
impl TurnObserver for Recorder {
    fn on_event(&self, event: &TurnObservation) {
        self.0.lock().unwrap().push(format!("{event:?}"));
    }
    fn on_turn(&self, trace: &TurnTrace<'_>) {
        self.0.lock().unwrap().push(format!("{trace:?}"));
    }
}
#[test]
fn terminal_errors_and_inputs_are_private_unless_content_is_requested() {
    common::runtime().block_on(async {
        let observer = Arc::new(Recorder::default());
        let result = observe_turn(
            observer.clone(),
            TraceContent::default(),
            "session",
            "SECRET-PROMPT",
            async {
                Err(CoreError::Rpc {
                    method: "test",
                    message: "SECRET-ERROR".into(),
                })
            },
        )
        .await;
        assert!(result.is_err());
        {
            let records = observer.0.lock().unwrap();
            assert_eq!(records.len(), 1);
            assert!(!records[0].contains("SECRET"));
            assert!(records[0].contains("Provider"));
        }
        observe_turn(
            observer.clone(),
            TraceContent::Include,
            "session",
            "SECRET-PROMPT",
            async {
                Err(CoreError::Rpc {
                    method: "test",
                    message: "SECRET-ERROR".into(),
                })
            },
        )
        .await
        .unwrap_err();
        let records = observer.0.lock().unwrap();
        assert!(records[1].contains("SECRET-PROMPT"));
        assert!(!records[1].contains("SECRET-ERROR"));
    });
}

#[cfg(feature = "langfuse")]
#[test]
fn existing_langfuse_exporter_builds_a_host_owned_batch_without_network() {
    use openhuman_embed::observe::langfuse::{LangfuseClient, LangfuseScore};
    let client = LangfuseClient::proxy("http://127.0.0.1:1", "fixture-token").unwrap();
    let batch = client.build_score_batch(LangfuseScore::numeric("run", "review", 1.0));
    assert_eq!(batch["batch"][0]["type"], "score-create");
    assert_eq!(batch["batch"][0]["body"]["traceId"], "run");
    assert!(!batch.to_string().contains("fixture-token"));
}
