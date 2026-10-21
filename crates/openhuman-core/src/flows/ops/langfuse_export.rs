//! Langfuse export for `flows::` graph runs.
//!
//! After a `flows_run` / `flows_resume` settles, the `flows::` domain hands
//! this module the run's [`GraphObservation`] slice collected from the Runs
//! bus event stream, and it exports the run as one Langfuse trace via the backend's Langfuse
//! OTLP proxy route (Langfuse v4; the legacy batch ingestion path is not used
//! for traces).
//!
//! The trace is built by `tinyagents`' [`GraphLangfuseExporter`], which turns
//! each superstep and node into a timed span and stamps the Langfuse **Agent
//! Graph view** keys (`langgraph_node` / `langgraph_step`) on node spans, so
//! the Langfuse UI can render the flow run as a graph, then projects it to
//! v4 OTLP: trace name, session, tags, release and flow metadata ride on every
//! span, since v4 filters observations directly. A host span-metadata
//! injector additionally stamps `flow_id` on every span for filtering.
//!
//! Transport mirrors the agent-turn tracing path
//! (`core::observability::progress_tracing::otlp::push_spans`): the endpoint is derived
//! from the **current backend hostname** (`effective_backend_api_url`), auth
//! is the live OpenHuman session bearer (the backend injects the real
//! Langfuse keys server-side, stamps the authenticated user on every span and
//! adds `x-langfuse-ingestion-version: 4`), the send is capped at 10s, a
//! partial OTLP rejection counts as a failure, and every failure is logged and
//! swallowed — exporting is best-effort and never fails the run. Gated on
//! `observability.share_usage_data`.

use std::time::Duration;

use serde_json::{json, Map, Value};
use tinyagents_graph::{GraphLangfuseExporter, GraphObservation};
use tinyagents_harness::{LangfuseAuth, LangfuseClient, LangfuseTraceConfig};
use tinyflows_bus::runs::FlowRunTrigger;

use crate::config::Config;
use crate::security::credentials::session_support::direct_backend_credential;

const LOG_TARGET: &str = "flows::langfuse";
/// Backend proxy route for legacy Langfuse ingestion (relative to the backend
/// origin). Only the client's base: [`LangfuseClient::otlp_endpoint`] derives
/// the OTLP route beside it, which is
/// where flow traces go. The backend authenticates the session JWT, injects
/// the Langfuse project keys, and forwards to Langfuse's
/// `/api/public/otel/v1/traces`.
/// Cap the push so a slow/hung Langfuse never stalls run teardown (same
/// posture as the agent-turn exporter).
const PUSH_TIMEOUT: Duration = Duration::from_secs(10);

fn trigger_name(trigger: FlowRunTrigger) -> &'static str {
    match trigger {
        FlowRunTrigger::Rpc => "rpc",
        FlowRunTrigger::Schedule => "schedule",
        FlowRunTrigger::AppEvent => "app_event",
        FlowRunTrigger::Resume => "resume",
    }
}

/// Resolve the Langfuse ingestion URL from the current backend host — the
/// exact base-server resolution every other backend call uses, so the host
/// always matches wherever the app's domain calls go (staging, prod, or a
/// custom `api_url` override).
fn ingestion_url(config: &Config) -> String {
    // Empty without a resolved ingestion URL; callers treat a non-`http` URL
    // as "proxy unavailable" and skip the push.
    config
        .resolved_endpoints
        .tracing_ingest_url
        .clone()
        .unwrap_or_default()
}

/// The OpenHuman core crate version (e.g. `0.58.0`), stamped onto every flow
/// trace as the Langfuse `release` field plus an `app_version` metadata key so
/// traces can be correlated with the app build that produced them.
const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Builds the [`LangfuseTraceConfig`] for one flow run: the trace id is the
/// run's `thread_id` (`flow:{flow_id}:{uuid}`), while the session id is the
/// stable `flow:{flow_id}` so repeated runs group together. The trace
/// is named `flow.run:{flow_name}`, run-type tags (`run:flow` +
/// `trigger:<kind>`) mark how the run started, and flow coordinates plus the
/// app version ride on the trace metadata. No content — ids, name, status,
/// trigger, and version only.
fn build_flow_trace_config(
    flow_name: &str,
    flow_id: &str,
    thread_id: &str,
    status: &str,
    trigger: FlowRunTrigger,
) -> LangfuseTraceConfig {
    let trigger = trigger_name(trigger);
    LangfuseTraceConfig {
        trace_id: Some(thread_id.to_string()),
        name: Some(format!("flow.run:{flow_name}")),
        session_id: Some(format!("flow:{flow_id}")),
        release: Some(APP_VERSION.to_string()),
        tags: vec!["run:flow".to_string(), format!("trigger:{trigger}")],
        metadata: json!({
            "flow_id": flow_id,
            "status": status,
            "source": "flows",
            "run_type": "flow",
            "trigger": trigger,
            "app_version": APP_VERSION,
        }),
        ..Default::default()
    }
}

/// Builds the graph exporter for a flow run: the batch builder plus a host
/// span-metadata injector that stamps `flow_id` on every span (node spans
/// already carry `langgraph_node`/`langgraph_step` from `tinyagents`).
fn build_flow_exporter(client: LangfuseClient, flow_id: &str) -> GraphLangfuseExporter {
    let flow_id = flow_id.to_string();
    GraphLangfuseExporter::new(client).with_span_metadata_fn(move |_obs| {
        let mut extra = Map::new();
        extra.insert("flow_id".to_string(), json!(flow_id));
        Some(extra)
    })
}

/// The Langfuse v4 OTLP requests for one flow run, under the product's
/// `service.name` (the same one agent-turn traces use).
fn flow_otlp_requests(
    exporter: &GraphLangfuseExporter,
    trace: LangfuseTraceConfig,
    observations: &[GraphObservation],
) -> tinyagents_harness::error::Result<Vec<Value>> {
    crate::core::observability::progress_tracing::build_graph_otlp_requests(
        exporter,
        trace,
        observations,
    )
}

/// Exports one settled flow run to Langfuse as a single trace. Best-effort:
/// every failure path logs a `[flows]`-prefixed warning and returns — a
/// Langfuse outage can never fail or delay-fail the run. No-op when
/// `observability.share_usage_data` is off or there is nothing to send.
pub async fn export_flow_run_trace(
    config: &Config,
    flow_name: &str,
    flow_id: &str,
    thread_id: &str,
    status: &str,
    trigger: FlowRunTrigger,
    observations: &[GraphObservation],
) {
    if !config.observability.share_usage_data {
        tracing::debug!(
            target: LOG_TARGET,
            flow_id = %flow_id,
            "[flows] langfuse export skipped: observability.share_usage_data is off"
        );
        return;
    }
    if observations.is_empty() {
        tracing::debug!(
            target: LOG_TARGET,
            flow_id = %flow_id,
            thread_id = %thread_id,
            "[flows] langfuse export skipped: PollRun observations are empty"
        );
        return;
    }

    let url = ingestion_url(config);
    if !url.starts_with("http") {
        tracing::warn!(
            target: LOG_TARGET,
            flow_id = %flow_id,
            "[flows] langfuse export skipped: could not resolve ingestion URL from backend host (got {url:?})"
        );
        return;
    }
    // No TinyHumans connection, or no usable credential (signed out, offline
    // local session): a configured state — `direct_backend_credential` logs
    // the reason at debug, and the export is skipped without a request.
    let token = match direct_backend_credential(config, "flows langfuse export") {
        Some(crate::security::credentials::session_support::BackendCredential::Session(token)) => {
            token
        }
        _ => {
            tracing::debug!(
                target: LOG_TARGET,
                flow_id = %flow_id,
                "[flows] langfuse export skipped: hosted backend not available"
            );
            return;
        }
    };
    let client = match LangfuseClient::new(url.clone(), LangfuseAuth::Bearer { token }) {
        Ok(client) => client,
        Err(err) => {
            tracing::warn!(
                target: LOG_TARGET,
                flow_id = %flow_id,
                error = %err,
                "[flows] langfuse export skipped: could not build client for {url}"
            );
            return;
        }
    };

    let exporter = build_flow_exporter(client, flow_id);
    let trace = build_flow_trace_config(flow_name, flow_id, thread_id, status, trigger);
    let observation_count = observations.len();
    tracing::debug!(
        target: LOG_TARGET,
        flow_id = %flow_id,
        thread_id = %thread_id,
        status = %status,
        trigger = %trigger_name(trigger),
        observation_count,
        endpoint = %exporter.client().otlp_endpoint(),
        "[flows] pushing flow run trace to Langfuse"
    );

    let requests = match flow_otlp_requests(&exporter, trace, observations) {
        Ok(requests) => requests,
        Err(err) => {
            tracing::warn!(
                target: LOG_TARGET,
                flow_id = %flow_id,
                thread_id = %thread_id,
                error = %err,
                "[flows] langfuse export skipped: could not build OTLP requests"
            );
            return;
        }
    };
    // The core observability owner performs the generic graph projection and
    // authenticated sends; this adapter supplies flow-specific trace metadata.
    let send =
        crate::core::observability::progress_tracing::send_graph_otlp_requests(&exporter, requests);
    match tokio::time::timeout(PUSH_TIMEOUT, send).await {
        Ok(Ok(request_count)) => {
            tracing::debug!(
                target: LOG_TARGET,
                flow_id = %flow_id,
                thread_id = %thread_id,
                observation_count,
                request_count,
                "[flows] pushed flow run trace to Langfuse"
            );
        }
        Ok(Err(err)) => {
            tracing::warn!(
                target: LOG_TARGET,
                flow_id = %flow_id,
                thread_id = %thread_id,
                error = %err,
                "[flows] langfuse export failed (run unaffected)"
            );
        }
        Err(_elapsed) => {
            tracing::warn!(
                target: LOG_TARGET,
                flow_id = %flow_id,
                thread_id = %thread_id,
                timeout_secs = PUSH_TIMEOUT.as_secs(),
                "[flows] langfuse export timed out (run unaffected)"
            );
        }
    }
}

#[cfg(test)]
#[path = "langfuse_export_tests.rs"]
mod tests;
