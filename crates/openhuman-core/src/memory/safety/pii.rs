//! Multilingual national-ID and personal-PII redaction.
//!
//! Split out of the original `safety.rs` (see [`super`]'s doc comment for
//! why this policy lives in OpenHuman rather than the memory engine).
//! Three responsibilities, three files:
//!
//! - [`checksums`] — Luhn / IBAN mod-97 / Verhoeff / CPF / CNPJ / CUIT /
//!   Spanish DNI-NIE structural validators, with no regex or normalization
//!   dependency.
//! - [`normalize`] — the Unicode normalization pass and the cheap
//!   byte-oriented candidate pre-filter that decides which precise regexes
//!   are even worth running.
//! - [`patterns`] — the per-identifier regexes, candidate-gated match
//!   collection, and the `redact_pii`/`has_likely_pii`/`has_likely_email`
//!   entry points this module re-exports.
//!
//! The generic secret-pattern scrubber and the `Sanitized`/
//! `SanitizationReport` types live in [`super::secrets`].

mod checksums;
mod normalize;
mod patterns;

pub use patterns::{has_likely_email, has_likely_pii, redact_pii};

// Flattened test-only re-exports so `safety_tests.rs` and its own submodules
// can exercise the internals directly (checksum validators, the
// normalization pass) the same way they could when everything lived in one
// `include!`-spliced scope.
#[cfg(test)]
pub(super) use checksums::{
    digits, valid_cnpj, valid_cpf, valid_cuit, valid_dni_es, valid_iban, valid_luhn, valid_nie_es,
    valid_nino, valid_ssn, valid_verhoeff,
};
#[cfg(test)]
pub(super) use normalize::{scan_candidates, NormalizedView};
#[cfg(test)]
pub(super) use patterns::{
    PII_AADHAAR, PII_CC, PII_CNPJ, PII_CPF, PII_CUIT, PII_DNI, PII_IBAN, PII_MYNUM, PII_NINO,
    PII_PAN_IN, PII_PHONE, PII_RFC, PII_RRN, PII_SSN,
};
