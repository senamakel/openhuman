//! Native trace export for completed TinyAgents runs.
//!
//! OpenHuman gates and sends native TinyAgents trace exports.

mod export;

#[cfg(test)]
#[path = "trace_export_adoption_tests.rs"]
mod adoption_tests;

use serde_json::Value;
use tinyagents_graph::{GraphLangfuseExporter, GraphObservation};
use tinyagents_harness::observability::trace_export::ExportBrand;
use tinyagents_harness::observability::LangfuseTraceConfig;

/// Product identity stamped onto exported telemetry.
pub(crate) fn export_brand() -> ExportBrand<'static> {
    ExportBrand {
        product: "openhuman",
        version: env!("CARGO_PKG_VERSION"),
    }
}

#[cfg(test)]
pub(crate) use export::build_subagent_trace;
pub(crate) use export::{export_run_trace_from_journal, export_subagent_journal_trace};

/// Build the generic graph-to-OTLP projection with OpenHuman's product name.
/// Flow identity and metadata stay with the flow adapter; this layer owns the
/// shared projection boundary used by both durable graph event sources.
pub(crate) fn build_graph_otlp_requests(
    exporter: &GraphLangfuseExporter,
    trace: LangfuseTraceConfig,
    observations: &[GraphObservation],
) -> tinyagents_harness::error::Result<Vec<Value>> {
    exporter.build_otlp_requests(trace, observations, export_brand().product)
}

/// Send a generic graph trace through the exporter’s authenticated OTLP
/// client. Callers own timeout and best-effort policy for their run lifecycle.
pub(crate) async fn send_graph_otlp_requests(
    exporter: &GraphLangfuseExporter,
    requests: Vec<Value>,
) -> tinyagents_harness::error::Result<usize> {
    let count = requests.len();
    for request in requests {
        exporter.client().send_otlp(request).await?;
    }
    Ok(count)
}
