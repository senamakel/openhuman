use super::*;
use tinyagents_harness::turn_outcome::ToolCallOutcome;

#[test]
fn outcome_source_returns_captured_content_by_call_id() {
    let sink: ToolOutcomeSink = Default::default();
    let source = OutcomeSinkSource(sink.clone());
    assert!(source.content_for("missing").unwrap().is_none());
    sink.lock().unwrap().push(ToolCallOutcome {
        call_id: "c1".into(),
        name: "t".into(),
        arguments: serde_json::Value::Null,
        success: true,
        content: "body".into(),
        duration_ms: 0,
    });
    assert_eq!(source.content_for("c1").unwrap().as_deref(), Some("body"));
}

#[test]
fn budget_notice_thresholds_stay_at_half_and_eighty_percent() {
    assert_eq!(BUDGET_NOTICE_THRESHOLDS, [0.5, 0.8]);
}

/// Records each call it gets, then fails: the tests read the order only.
struct RecordingSummarizer(Arc<std::sync::Mutex<Vec<&'static str>>>);

#[async_trait]
impl Summarizer for RecordingSummarizer {
    async fn summarize(&self, _: &[Message]) -> tinyagents_harness::error::Result<SummaryRecord> {
        self.0.lock().unwrap().push("summarize");
        Err(tinyagents_harness::error::TinyAgentsError::Model(
            "stub".into(),
        ))
    }

    async fn merge(&self, _: &[SummaryRecord]) -> tinyagents_harness::error::Result<SummaryRecord> {
        self.0.lock().unwrap().push("merge");
        Err(tinyagents_harness::error::TinyAgentsError::Model(
            "stub".into(),
        ))
    }
}

fn recording() -> (
    Arc<std::sync::Mutex<Vec<&'static str>>>,
    Box<dyn Summarizer>,
) {
    let calls = Arc::new(std::sync::Mutex::new(Vec::new()));
    let notice_calls = calls.clone();
    let notice: CompactionNotice = Arc::new(move || notice_calls.lock().unwrap().push("notice"));
    let summarizer = announced(Box::new(RecordingSummarizer(calls.clone())), Some(notice));
    (calls, summarizer)
}

#[tokio::test]
async fn a_summarization_is_announced_before_it_runs() {
    let (calls, summarizer) = recording();
    let _ = summarizer.summarize(&[]).await;
    let _ = summarizer
        .summarize_request(&SummaryRequest::new(Vec::new()))
        .await;
    assert_eq!(
        *calls.lock().unwrap(),
        ["notice", "summarize", "notice", "summarize"]
    );
}

#[tokio::test]
async fn merging_summaries_is_not_announced_again() {
    let (calls, summarizer) = recording();
    let _ = summarizer.merge(&[]).await;
    assert_eq!(*calls.lock().unwrap(), ["merge"]);
}
