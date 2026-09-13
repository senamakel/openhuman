use super::*;
use serde_json::json;

use crate::memory::safety::pii::redact_pii;
// `pii`'s internals (checksum validators, the normalization pass) are test-only
// re-exports at the `pii` module level; pull them in here so the nested test
// submodules below can reach them through their own `use super::*;`.
use super::pii::{
    digits, scan_candidates, valid_cnpj, valid_cpf, valid_cuit, valid_dni_es, valid_iban,
    valid_luhn, valid_nie_es, valid_nino, valid_ssn, valid_verhoeff, NormalizedView,
};
use super::secrets::{MAX_JSON_SANITIZE_DEPTH, REDACTED_PRIVATE_KEY, REDACTED_SECRET};

/// Assembled rather than written out so a repository secret scanner does
/// not read the fixture as a real key block.
fn private_key_fixture(kind: &str, body: &str) -> String {
    format!("-----BEGIN {kind}-----\n{body}\n-----END {kind}-----")
}

fn redacts(input: &str, token: &str) {
    let out = redact_pii(input);
    assert!(
        out.value.contains(token),
        "expected {token} in output. input={input:?} output={out:?}"
    );
}

fn unchanged(input: &str) {
    let out = redact_pii(input);
    assert_eq!(
        out.value, input,
        "expected no change; report={:?}",
        out.report
    );
    assert_eq!(out.report.pii_redactions, 0);
}

#[path = "prefilter_and_checksum_tests.rs"]
mod prefilter_and_checksum_tests;
#[path = "sanitize_and_pii_id_tests.rs"]
mod sanitize_and_pii_id_tests;
