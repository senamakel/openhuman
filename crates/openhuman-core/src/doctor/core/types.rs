//! The doctor report's data shapes: diagnostic severities, entries, and the
//! model-probe report types.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Severity {
    Ok,
    Warn,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticItem {
    pub severity: Severity,
    pub category: String,
    pub message: String,
}

impl DiagnosticItem {
    pub(crate) fn ok(category: impl Into<String>, msg: impl Into<String>) -> Self {
        Self {
            severity: Severity::Ok,
            category: category.into(),
            message: msg.into(),
        }
    }
    pub(crate) fn warn(category: impl Into<String>, msg: impl Into<String>) -> Self {
        Self {
            severity: Severity::Warn,
            category: category.into(),
            message: msg.into(),
        }
    }
    pub(crate) fn error(category: impl Into<String>, msg: impl Into<String>) -> Self {
        Self {
            severity: Severity::Error,
            category: category.into(),
            message: msg.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoctorSummary {
    pub ok: usize,
    pub warnings: usize,
    pub errors: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoctorReport {
    pub items: Vec<DiagnosticItem>,
    pub summary: DoctorSummary,
}

// ── Public entry point ───────────────────────────────────────────
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ModelProbeOutcome {
    Ok,
    Skipped,
    AuthOrAccess,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelProbeEntry {
    pub provider: String,
    pub outcome: ModelProbeOutcome,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelProbeSummary {
    pub ok: usize,
    pub skipped: usize,
    pub auth_or_access: usize,
    pub errors: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelProbeReport {
    pub entries: Vec<ModelProbeEntry>,
    pub summary: ModelProbeSummary,
}
