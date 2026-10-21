//! Output-budget capability tests for live inference checks.

use super::inference_live::{probe_prepared_model, LiveProbeCache};
use crate::doctor::FindingSeverity;
use crate::inference::provider::factory::{
    InspectionOutputBudgetCapability, InspectionRequestIdentity, PreparedInspectionChatModel,
};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tinyinference_llm::model::{ChatModel, ModelRequest, ModelResponse};

#[tokio::test]
async fn unsupported_output_budget_is_skipped_without_invoking_the_model() {
    struct InvocationCounter(Arc<AtomicUsize>);

    #[async_trait::async_trait]
    impl ChatModel<()> for InvocationCounter {
        async fn invoke(
            &self,
            _state: &(),
            _request: ModelRequest,
        ) -> tinyinference_llm::Result<ModelResponse> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(ModelResponse::assistant("OK"))
        }
    }

    let invocations = Arc::new(AtomicUsize::new(0));
    let prepared = PreparedInspectionChatModel {
        model: Arc::new(InvocationCounter(Arc::clone(&invocations))),
        model_id: "gpt-codex".into(),
        identity: InspectionRequestIdentity::new(
            "https://chatgpt.com/backend-api/codex".into(),
            "dummy-frozen-codex-token".into(),
            "gpt-codex".into(),
        ),
        output_budget: InspectionOutputBudgetCapability::Unsupported,
    };
    let finding =
        probe_prepared_model(&LiveProbeCache::default(), prepared, "inference/live/chat").await;
    let invocation_count = invocations.load(Ordering::SeqCst);

    assert!(
        finding.severity == FindingSeverity::Skipped && invocation_count == 0,
        "a route without an enforced output limit must skip before invoking; severity={:?}, invocations={invocation_count}",
        finding.severity
    );
}
