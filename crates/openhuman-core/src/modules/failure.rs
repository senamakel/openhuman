//! Sanitized terminal module reports, deduplicated for the process lifetime.

use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

use super::types::ModuleRecord;

/// Closed reason vocabulary: never accept a module error, path or payload here.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(super) enum Reason {
    Disabled,
    LoaderDisabled,
    ResolutionFailed,
    IncompatibleContract,
    TransportFailed,
    ModuleFault,
}

impl Reason {
    fn code(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::LoaderDisabled => "loader_disabled",
            Self::ResolutionFailed => "resolution_failed",
            Self::IncompatibleContract => "incompatible_contract",
            Self::TransportFailed => "transport_failed",
            Self::ModuleFault => "module_fault",
        }
    }

    fn stage(self) -> &'static str {
        match self {
            Self::Disabled | Self::LoaderDisabled => "availability",
            Self::ResolutionFailed => "resolve",
            Self::IncompatibleContract => "contract",
            Self::TransportFailed => "transport",
            Self::ModuleFault => "execution",
        }
    }
}

/// Report a terminal outcome once. Registry records supply all identifying tags.
///
/// There are finitely many registry records and reasons, so the process cache
/// is bounded. Concurrent callers cannot duplicate the same terminal report.
pub(super) fn report(record: &'static ModuleRecord, reason: Reason) {
    static REPORTED: OnceLock<Mutex<HashSet<(&'static str, Reason)>>> = OnceLock::new();
    let first = REPORTED
        .get_or_init(|| Mutex::new(HashSet::new()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert((record.id, reason));
    if !first {
        return;
    }
    let platform = format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH);
    let capture = || {
        crate::core::observability::report_error(
            "loadable module operation failed",
            "modules",
            reason.stage(),
            &[
                ("module", record.id),
                ("version", record.version),
                ("stage", reason.stage()),
                ("platform", &platform),
                ("reason_code", reason.code()),
            ],
        )
    };
    // A host may have put request payloads or user paths on its current scope.
    // These terminal events carry only the closed metadata above.
    #[cfg(feature = "crash-reporting")]
    sentry::with_scope(|scope| scope.clear(), capture);
    #[cfg(not(feature = "crash-reporting"))]
    capture();
}

#[cfg(test)]
#[path = "failure_tests.rs"]
mod tests;
