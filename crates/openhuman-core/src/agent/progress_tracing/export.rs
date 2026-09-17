//! Handing finished spans to their sink(s): the local NDJSON/log exporter and
//! the remote Langfuse usage-data push.

use crate::config::schema::AgentTracingConfig;
use crate::config::Config;

use super::langfuse;
use super::serialize::spans_to_ndjson;
use super::types::{TraceContext, TraceSpan};

/// Export finished spans per the [`AgentTracingConfig`]: append NDJSON to the
/// configured file, or emit to the application log when no path is set.
/// Best-effort — a failed write is logged and swallowed so tracing never
/// breaks an agent run. A no-op when tracing is disabled or there are no spans.
pub(crate) fn export_spans(config: &AgentTracingConfig, spans: &[TraceSpan]) {
    if !config.enabled || spans.is_empty() {
        return;
    }
    let payload = spans_to_ndjson(config.backend, spans);
    match &config.export_path {
        Some(path) => {
            use std::io::Write as _;
            let opened = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path);
            match opened {
                Ok(mut file) => {
                    if let Err(err) = file.write_all(payload.as_bytes()) {
                        log::warn!(
                            "[agent-tracing] failed to append {} spans to {path}: {err}",
                            spans.len()
                        );
                    } else {
                        log::debug!("[agent-tracing] exported {} spans to {path}", spans.len());
                    }
                }
                Err(err) => log::warn!("[agent-tracing] failed to open {path}: {err}"),
            }
        }
        None => {
            // No path configured. Surface only metadata (count + trace id) at
            // `info` so the export is visible on read-only / sandboxed
            // deployments WITHOUT ever printing span content at `info` (#4454).
            // The NDJSON body — which may carry prompt/reply text when
            // `capture_content` is opted in — goes to `debug` only. With the
            // default gate off, the storage layer already strips content, so
            // `payload` is metadata-only regardless.
            log::info!(
                "[agent-tracing] {} spans (trace_id={}) — set observability.agent_tracing.export_path to persist",
                spans.len(),
                spans.first().map(|s| s.trace_id.as_str()).unwrap_or(""),
            );
            log::debug!(
                target: "agent-tracing",
                "[agent-tracing] span NDJSON ({} spans):\n{}",
                spans.len(),
                payload.trim_end()
            );
        }
    }
}

/// Hand a completed run's spans to the configured tracing sink(s).
///
/// Two independent paths, both best-effort and never fatal to a turn:
///
/// 1. **Usage-data sharing** (`observability.share_usage_data`, on by default):
///    push the run's spans to the backend Langfuse proxy — endpoint derived from
///    the current backend host, authed with the session bearer (see
///    [`langfuse::push_spans`]). A failure (no live session, network, rejected
///    batch) just logs; there is no local fallback, since sharing and local
///    export are distinct opt-ins. Web-channel turns that successfully read a
///    durable tinyagents journal should call [`export_run_trace_from_journal`]
///    instead, so the remote push uses the crate-owned observation exporter.
/// 2. **Local exporter** (`observability.agent_tracing.enabled`, opt-in): append
///    OTel/Langfuse-format NDJSON to the configured file or the app log via
///    [`export_spans`].
///
/// A no-op when there are no spans or both paths are off.
pub(crate) async fn export_run_trace(config: &Config, spans: &[TraceSpan]) {
    if spans.is_empty() {
        return;
    }
    let observability = &config.observability;

    if observability.share_usage_data {
        if let Err(err) = langfuse::push_spans(config, spans).await {
            log::warn!("[agent-tracing] Langfuse usage-data push failed ({err})");
        }
    }

    if observability.agent_tracing.enabled {
        export_spans(&observability.agent_tracing, spans);
    }
}

/// Export a completed run when durable tinyagents observations are available.
/// Remote usage-data sharing uses the crate Langfuse exporter over the journal;
/// local tracing still writes the live spans until the migration deletes the
/// legacy span collector/exporter path.
pub(crate) async fn export_run_trace_from_journal(
    config: &Config,
    trace_ctx: &TraceContext,
    observations: &[tinyagents_harness::observability::AgentObservation],
    run_telemetry: Option<&tinyagents_session::run_ledger::RunTelemetry>,
    live_spans: &[TraceSpan],
) {
    if observations.is_empty() && live_spans.is_empty() {
        return;
    }
    let observability = &config.observability;

    if observability.share_usage_data && !observations.is_empty() {
        if let Err(err) =
            langfuse::push_observations(config, trace_ctx, observations, run_telemetry).await
        {
            log::warn!("[agent-tracing] Langfuse journal usage-data push failed ({err})");
        }
    } else if observability.share_usage_data {
        log::debug!("[agent-tracing] no journal observations for Langfuse usage-data push");
    }

    if observability.agent_tracing.enabled && !live_spans.is_empty() {
        export_spans(&observability.agent_tracing, live_spans);
    }
}
