//! Secret and PII scrubbing for anything this host persists or hands on.
//!
//! Conservative by design — it prefers false positives over leaking a
//! credential into a long-lived store.
//!
//! # Why this is OpenHuman's and not the engine's
//!
//! Which byte sequences count as a credential worth blanking, and which
//! national-ID shapes are worth a checksum, is product policy about what this
//! host is willing to write down — the same kind of policy as the preference
//! lanes, the archivist's event patterns and the log-redaction hash before it.
//! Ten call sites reached for it through `tinymemory_core::store::safety`,
//! which is an engine dependency taken on for a page of regexes: none of them
//! is a memory read or a memory write, and several — approval records,
//! tool-result artifacts, offloaded artifacts — never touch memory at all.
//!
//! Widening `tinymemory-api` to carry it instead would be the wrong trade, for
//! the reason [`crate::openhuman::util::redact`] records: the contract crate's
//! manifest documents that it stays dependency-light so a caller can depend on
//! it and compile almost nothing, and this policy costs `regex` plus
//! `serde_json`. Nor is it contract vocabulary — nothing crosses the bus as a
//! [`SanitizationReport`], and a scrubber is not a capability a second memory
//! driver would implement differently.
//!
//! The engine keeps its own copy for its own writes. The two are independent by
//! design: neither reads the other's output, and the `[REDACTED_*]` placeholders
//! are markers for a human reading the row, not wire values that have to agree
//! across a boundary. Divergence between the copies changes what each redacts,
//! never whether a value round-trips.
//!
//! # Ported verbatim
//!
//! Same pattern lists, same replacement tokens, same 128-level JSON depth cap,
//! same sensitive-key classifier, same checksum gates, and the same priority and
//! overlap-resolution order — so every caller stores exactly the bytes it always
//! stored. The engine's module tree (`safety` over `pii` over
//! `checks`/`normalize`/`prefilter`) is flat here because nothing outside it ever
//! named the inner modules; every item keeps its name, so the two copies stay
//! diffable line for line.

use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;

const REDACTED_SECRET: &str = "[REDACTED_SECRET]";
const REDACTED_PRIVATE_KEY: &str = "[REDACTED_PRIVATE_KEY]";
const MAX_JSON_SANITIZE_DEPTH: usize = 128;

/// Tally of what a sanitization pass changed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SanitizationReport {
    /// Count of secret/token pattern matches rewritten in string text by the
    /// text-pattern redaction pass.
    pub text_redactions: usize,
    /// Count of JSON object entries dropped wholesale because their key was
    /// classified as sensitive by the key classifier.
    pub key_redactions: usize,
    /// Count of full private-key blocks replaced; these are
    /// the most severe hits since the entire block is removed.
    pub blocked_secret_hits: usize,
    /// Count of nodes collapsed because JSON nesting reached
    /// the JSON traversal depth cap; the subtree is replaced rather than walked.
    pub depth_redactions: usize,
    /// Count of personal-identifier matches replaced by the
    /// lightweight PII screen.
    pub pii_redactions: usize,
}

impl SanitizationReport {
    /// True when any field recorded a redaction.
    pub fn changed(&self) -> bool {
        self.text_redactions > 0
            || self.key_redactions > 0
            || self.blocked_secret_hits > 0
            || self.depth_redactions > 0
            || self.pii_redactions > 0
    }

    /// Sum two reports field-wise.
    pub fn merge(self, rhs: Self) -> Self {
        Self {
            text_redactions: self.text_redactions + rhs.text_redactions,
            key_redactions: self.key_redactions + rhs.key_redactions,
            blocked_secret_hits: self.blocked_secret_hits + rhs.blocked_secret_hits,
            depth_redactions: self.depth_redactions + rhs.depth_redactions,
            pii_redactions: self.pii_redactions + rhs.pii_redactions,
        }
    }
}

/// A sanitized value plus the [`SanitizationReport`] describing the changes.
#[derive(Debug, Clone)]
pub struct Sanitized<T> {
    /// The cleaned value with secrets and PII removed.
    pub value: T,
    /// Tally of what the sanitization pass changed to produce `value`.
    pub report: SanitizationReport,
}

static BLOCK_PATTERNS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    vec![
        Regex::new(
            r"(?is)-----BEGIN(?: [A-Z]+)? PRIVATE KEY-----.*?-----END(?: [A-Z]+)? PRIVATE KEY-----",
        )
        .expect("valid private key block"),
        Regex::new(r"(?is)-----BEGIN OPENSSH PRIVATE KEY-----.*?-----END OPENSSH PRIVATE KEY-----")
            .expect("valid openssh private key block"),
        Regex::new(
            r"(?is)-----BEGIN PGP PRIVATE KEY BLOCK-----.*?-----END PGP PRIVATE KEY BLOCK-----",
        )
        .expect("valid pgp private key block"),
    ]
});

static REDACTION_PATTERNS: LazyLock<Vec<(Regex, &'static str)>> = LazyLock::new(|| {
    vec![
        (
            Regex::new(r"(?i)(bearer\s+)[A-Za-z0-9._~+/=-]{8,}").expect("valid bearer redaction"),
            "${1}[REDACTED]",
        ),
        (
            Regex::new(r#"(?i)(api[_-]?key\s*[=:\s]\s*["']?)[^\s"']+"#)
                .expect("valid api key redaction"),
            "${1}[REDACTED]",
        ),
        (
            Regex::new(
                r#"(?i)\b(token|access[_-]?token|refresh[_-]?token|client[_-]?secret|password|secret)\b\s*[=:\s]\s*["']?[^\s"'&]+"#,
            )
            .expect("valid token redaction"),
            "[REDACTED]",
        ),
        (
            Regex::new(r"\bsk-[A-Za-z0-9]{20,}\b").expect("valid openai key redaction"),
            "[REDACTED]",
        ),
        (
            Regex::new(r"\bgh[pousr]_[A-Za-z0-9_]{20,}\b").expect("valid github token redaction"),
            "[REDACTED]",
        ),
        (
            Regex::new(r"\bAKIA[0-9A-Z]{16}\b").expect("valid aws key redaction"),
            "[REDACTED]",
        ),
        (
            Regex::new(r"\bASIA[0-9A-Z]{16}\b").expect("valid aws sts key redaction"),
            "[REDACTED]",
        ),
        (
            Regex::new(r"\beyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9._-]{8,}\.[A-Za-z0-9._-]{8,}\b")
                .expect("valid jwt redaction"),
            "[REDACTED]",
        ),
        (
            Regex::new(
                r#"(?i)\b(access_token|refresh_token|id_token|authorization_code|code_verifier|code_challenge)\b\s*[=:\s]\s*["']?[^\s"'&]+"#,
            )
            .expect("valid oauth token redaction"),
            "[REDACTED]",
        ),
        (
            Regex::new(r"\bAIza[0-9A-Za-z\-_]{35}\b").expect("valid google api key redaction"),
            "[REDACTED]",
        ),
        (
            Regex::new(r"\bsk-ant-[A-Za-z0-9\-_]{16,}\b").expect("valid anthropic key redaction"),
            "[REDACTED]",
        ),
        (
            Regex::new(r"\bsk-(?:proj|org)-[A-Za-z0-9\-_]{12,}\b")
                .expect("valid openai scoped key redaction"),
            "[REDACTED]",
        ),
        (
            Regex::new(r"\b(?:sk|rk)_(?:live|test)_[A-Za-z0-9]{16,}\b")
                .expect("valid stripe key redaction"),
            "[REDACTED]",
        ),
        (
            Regex::new(r"\bxox(?:a|b|p|s|r)-[A-Za-z0-9-]{10,}\b")
                .expect("valid slack token redaction"),
            "[REDACTED]",
        ),
        (
            Regex::new(r"\bgithub_pat_[A-Za-z0-9_]{20,}\b").expect("valid github pat redaction"),
            "[REDACTED]",
        ),
        (
            Regex::new(r"\bglpat-[A-Za-z0-9\-_]{16,}\b").expect("valid gitlab pat redaction"),
            "[REDACTED]",
        ),
        (
            Regex::new(r"\bnpm_[A-Za-z0-9]{20,}\b").expect("valid npm token redaction"),
            "[REDACTED]",
        ),
        (
            Regex::new(r"\bSG\.[A-Za-z0-9_\-]{16,}\.[A-Za-z0-9_\-]{16,}\b")
                .expect("valid sendgrid key redaction"),
            "[REDACTED]",
        ),
    ]
});

/// True when `value` looks like it contains a credential.
pub fn has_likely_secret(value: &str) -> bool {
    BLOCK_PATTERNS.iter().any(|p| p.is_match(value))
        || REDACTION_PATTERNS.iter().any(|(p, _)| p.is_match(value))
}

/// Scrub secrets and PII from free text, returning the cleaned text plus a
/// [`SanitizationReport`].
pub fn sanitize_text(value: &str) -> Sanitized<String> {
    let mut out = value.to_string();
    let mut report = SanitizationReport::default();

    for pattern in BLOCK_PATTERNS.iter() {
        let hits = pattern.find_iter(&out).count();
        if hits > 0 {
            report.blocked_secret_hits += hits;
            out = pattern.replace_all(&out, REDACTED_PRIVATE_KEY).into_owned();
        }
    }

    for (pattern, replacement) in REDACTION_PATTERNS.iter() {
        let hits = pattern.find_iter(&out).count();
        if hits > 0 {
            report.text_redactions += hits;
            out = pattern.replace_all(&out, *replacement).into_owned();
        }
    }

    // Full multilingual national-ID PII scrub (checksum-gated, normalization
    // pre-pass) — runs after secret redaction so every call site that scrubs
    // secrets also scrubs PII.
    let pii = redact_pii(&out);
    report = report.merge(pii.report);
    out = pii.value;

    Sanitized { value: out, report }
}

/// Recursively scrub a JSON value: sensitive keys are replaced wholesale and
/// every string value runs through `sanitize_text`.
pub fn sanitize_json(value: &Value) -> Sanitized<Value> {
    sanitize_json_inner(value, 0)
}

/// Recursive worker behind [`sanitize_json`].
///
/// `depth` counts nesting from the call in `sanitize_json` (which starts at
/// `0`); once it reaches [`MAX_JSON_SANITIZE_DEPTH`] the whole subtree at that
/// point is replaced by a single redaction marker rather than walked further,
/// bounding recursion against pathologically deep or adversarial JSON.
fn sanitize_json_inner(value: &Value, depth: usize) -> Sanitized<Value> {
    if depth >= MAX_JSON_SANITIZE_DEPTH {
        return Sanitized {
            value: Value::String(REDACTED_SECRET.to_string()),
            report: SanitizationReport {
                depth_redactions: 1,
                ..SanitizationReport::default()
            },
        };
    }

    match value {
        Value::Object(map) => {
            let mut out = serde_json::Map::new();
            let mut report = SanitizationReport::default();
            for (key, value) in map {
                if is_sensitive_key(key) {
                    report.key_redactions += 1;
                    out.insert(key.clone(), Value::String(REDACTED_SECRET.to_string()));
                    continue;
                }
                let sanitized = sanitize_json_inner(value, depth + 1);
                report = report.merge(sanitized.report);
                out.insert(key.clone(), sanitized.value);
            }
            Sanitized {
                value: Value::Object(out),
                report,
            }
        }
        Value::Array(items) => {
            let mut out = Vec::with_capacity(items.len());
            let mut report = SanitizationReport::default();
            for item in items {
                let sanitized = sanitize_json_inner(item, depth + 1);
                report = report.merge(sanitized.report);
                out.push(sanitized.value);
            }
            Sanitized {
                value: Value::Array(out),
                report,
            }
        }
        Value::String(value) => {
            let sanitized = sanitize_text(value);
            Sanitized {
                value: Value::String(sanitized.value),
                report: sanitized.report,
            }
        }
        _ => Sanitized {
            value: value.clone(),
            report: SanitizationReport::default(),
        },
    }
}

/// True when a JSON object key's name itself suggests it holds a secret
/// (`api_key`, `token`, `password`, …), independent of the value's contents.
///
/// Matching keys are redacted wholesale in [`sanitize_json_inner`] — the
/// value is replaced rather than scanned, since a key named e.g. `password`
/// is assumed sensitive even if its value doesn't match any
/// [`REDACTION_PATTERNS`] regex. Matching is on the key with all
/// non-alphanumeric characters stripped and lowercased, so `API-Key`,
/// `api_key`, and `apiKey` are all treated identically.
fn is_sensitive_key(key: &str) -> bool {
    let normalized: String = key
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect();

    matches!(
        normalized.as_str(),
        "apikey"
            | "token"
            | "accesstoken"
            | "refreshtoken"
            | "authorization"
            | "password"
            | "secret"
            | "clientsecret"
    ) || normalized.ends_with("token")
        || normalized.ends_with("apikey")
        || normalized.ends_with("clientsecret")
        || normalized.contains("password")
        || normalized.contains("secret")
        || normalized.ends_with("key")
}

// ---------- Multilingual personal-PII redaction ----------
//
// On-device, regex + checksum only, zero network.
//
// Design — security first:
//
// 1. Checksum gating where possible. CPF, CNPJ, CUIT, credit-card (Luhn),
//    IBAN (mod-97), Aadhaar (Verhoeff), Spanish DNI/NIE (check letter), and
//    US SSN reserved-range filters all reject look-alikes that aren't real
//    identifiers. The false-positive rate from format alone is too high; the
//    checksums bring it back to acceptable.
//
// 2. Bypass-resistant. Inputs are normalized before matching, which:
//      - strips zero-width characters (U+200B/200C/200D/FEFF/2060/180E),
//      - folds fullwidth digits (`0-9` fullwidth to ASCII) and fullwidth
//        `.-/:` to their ASCII counterparts,
//      - folds Arabic-Indic and Eastern Arabic-Indic digits to ASCII.
//    Match offsets are mapped back to the original text so we only redact
//    the bytes that actually carry PII; surrounding text is untouched.
//
// 3. Overlap-safe. Patterns are run in priority order; later matches that
//    overlap an earlier redaction are dropped, so a credit-card span can't
//    also be partially matched as a phone number.
//
// 4. Out of scope. Contextual PII (`"call me at the usual number"`), compound
//    PII (`name + employer + city`), arbitrary names, and freeform dates-of-
//    birth all require NER/LLM and are NOT addressed here. This module is
//    honest about its scope.

// ---------- Replacement tokens ----------

const PII_RFC: &str = "[REDACTED_PII_RFC]";
const PII_CPF: &str = "[REDACTED_PII_CPF]";
const PII_CNPJ: &str = "[REDACTED_PII_CNPJ]";
const PII_CUIT: &str = "[REDACTED_PII_CUIT]";
const PII_MYNUM: &str = "[REDACTED_PII_MYNUMBER]";
const PII_PHONE: &str = "[REDACTED_PII_PHONE]";
const PII_SSN: &str = "[REDACTED_PII_SSN]";
const PII_CC: &str = "[REDACTED_PII_CREDIT_CARD]";
const PII_IBAN: &str = "[REDACTED_PII_IBAN]";
const PII_AADHAAR: &str = "[REDACTED_PII_AADHAAR]";
const PII_PAN_IN: &str = "[REDACTED_PII_PAN_IN]";
const PII_NINO: &str = "[REDACTED_PII_NINO]";
const PII_DNI: &str = "[REDACTED_PII_DNI]";
const PII_RRN: &str = "[REDACTED_PII_RRN]";

// ---------- Patterns ----------

// Brazilian CPF, formatted: NNN.NNN.NNN-NN
static CPF_FMT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b\d{3}\.\d{3}\.\d{3}-\d{2}\b").expect("cpf fmt"));
// Brazilian CPF, bare: 11 consecutive digits. Checksum-gated; ~1% raw FP.
static CPF_BARE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b\d{11}\b").expect("cpf bare"));

// Brazilian CNPJ, formatted: NN.NNN.NNN/NNNN-NN
static CNPJ_FMT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b\d{2}\.\d{3}\.\d{3}/\d{4}-\d{2}\b").expect("cnpj fmt"));
// Brazilian CNPJ, bare: 14 consecutive digits.
static CNPJ_BARE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b\d{14}\b").expect("cnpj bare"));

// Argentine CUIT/CUIL: NN-NNNNNNNN-N (formatted only — bare 11-digit with
// single check digit has ~9% FP on random IDs, too noisy without context).
static CUIT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b\d{2}-\d{8}-\d\b").expect("cuit"));

// Mexican RFC: 3-4 letters (incl. Ñ &) + 6 digits + 3 alphanumeric homoclave.
static RFC_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b[A-ZÑ&]{3,4}\d{6}[A-Z0-9]{3}\b").expect("rfc"));

// Japan My Number (12 digits) gated by a Japanese or English keyword within
// ~30 chars. Bare 12-digit runs without keyword are too noisy.
static MYNUM_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:マイナンバー|個人番号|My\s?Number)[\s:はがを、.\-]{0,12}(\d{12})\b")
        .expect("my number")
});

// E.164 phone: + followed by 7-15 digits, no separators.
static PHONE_E164_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\+\d{7,15}\b").expect("e164"));

// NANP (US/Canada) formatted phone. Area code must start 2-9; first digit of
// central-office code also 2-9 (real NANP rule).
static PHONE_NANP_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b(?:\+?1[\s.\-]?)?\(?([2-9]\d{2})\)?[\s.\-]?([2-9]\d{2})[\s.\-]?(\d{4})\b")
        .expect("nanp phone")
});

// US SSN: NNN-NN-NNNN. Range filter applied below.
static SSN_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b\d{3}-\d{2}-\d{4}\b").expect("ssn"));

// Credit card: 13-19 digits with optional spaces/dashes every 4. Luhn-gated.
static CC_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b(?:\d[\s\-]?){13,19}\b").expect("credit card"));

// IBAN: 2 letter country code + 2 check digits + 11-30 alphanumeric.
// Allow optional spaces every 4 chars (common human format).
static IBAN_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b[A-Z]{2}\d{2}(?:[\s]?[A-Z0-9]){11,30}\b").expect("iban"));

// India Aadhaar: 4-4-4 digit groups (space or hyphen) OR contiguous 12 digits
// gated by keyword. Verhoeff-checksum-gated when grouped, keyword-gated when
// bare (Verhoeff alone has ~10% raw FP rate on random 12-digit runs).
static AADHAAR_FMT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b\d{4}[\s\-]\d{4}[\s\-]\d{4}\b").expect("aadhaar formatted"));
static AADHAAR_KW_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?:aadhaar|aadhar|आधार|uidai|uid)[\s:#\-no.]{0,10}(\d{12})\b")
        .expect("aadhaar keyword")
});

// India PAN: 5 letters, 4 digits, 1 letter. Very high signal — no checksum.
static PAN_IN_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b[A-Z]{5}\d{4}[A-Z]\b").expect("pan-in"));

// UK NINO: 2 letters + 6 digits + suffix A/B/C/D.
static NINO_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b[A-Z]{2}\d{6}[A-D]\b").expect("nino"));

// Spain DNI: 8 digits + check letter. NIE: starts X/Y/Z, then 7 digits + letter.
static DNI_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\b\d{8}[A-Z]\b").expect("dni"));
static NIE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b[XYZ]\d{7}[A-Z]\b").expect("nie"));

// South Korea RRN: NNNNNN-CXXXXXX where C is gender/century digit (1-4).
static RRN_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b\d{6}-[1-4]\d{6}\b").expect("rrn"));
static EMAIL_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b[A-Z0-9._%+-]+@[A-Z0-9.-]+\.[A-Z]{2,}\b").expect("email"));

// ---------- Public API ----------

/// Redact format-based multilingual PII from `text`.
///
/// Runs a Unicode normalization pre-pass to defeat fullwidth-digit and
/// zero-width-char bypasses. Match indices from the normalized form are
/// translated back to original byte offsets so only the PII bytes are
/// replaced — surrounding text (including any preserved fullwidth glyphs)
/// is untouched.
pub fn redact_pii(text: &str) -> Sanitized<String> {
    let mut report = SanitizationReport::default();

    // Fast path: cheap byte pre-filter on the raw text. Fullwidth / Arabic-Indic
    // digits and folded punctuation only surface after normalization, so a clean
    // raw scan still re-checks the normalized view before declaring the text PII-
    // free (mirrors the old two-phase SCREEN check).
    let raw_cand = scan_candidates(text);
    if !raw_cand.any() {
        let nview = NormalizedView::build(text);
        let ncand = scan_candidates(&nview.normalized);
        if !ncand.any() {
            log::trace!(
                "[pii] redact_pii: no candidate before or after normalization (len={})",
                text.len()
            );
            return Sanitized {
                value: text.to_string(),
                report,
            };
        }
        log::debug!("[pii] redact_pii: candidate surfaced only after normalization");
        return splice_redactions(
            text,
            &nview,
            collect_redactions(&nview.normalized, &ncand),
            &mut report,
        );
    }

    let nview = NormalizedView::build(text);
    // Gate on candidates from the NORMALIZED text — the precise regexes run
    // against it, so normalization-induced classes (folded digits) are included.
    let ncand = scan_candidates(&nview.normalized);
    let redactions = collect_redactions(&nview.normalized, &ncand);
    splice_redactions(text, &nview, redactions, &mut report)
}

/// True if `value` looks like it carries any PII. Used to *reject*
/// namespace/key inputs at boundary checks (analogous to
/// [`has_likely_secret`]).
///
/// Uses the **strict** match set — only formatted / keyword-gated patterns.
/// Bare-numeric patterns whose only signal is a digit run (credit card via
/// Luhn, bare CPF, bare CNPJ) or a phone-shaped digit run (NANP without
/// separators, E.164 leading `+`) are excluded here because their false-
/// positive rate against scanner-built namespace/key identifiers (WhatsApp
/// JIDs like `12025551234-1543890267@g.us`, telegram numeric peer IDs,
/// millisecond timestamps, padded counters) is too high to use as a hard
/// rejection signal. Content scrubbing via [`redact_pii`] still applies
/// those patterns — false positives are tolerable there because they only
/// replace bytes inside a string, not reject the whole write.
pub fn has_likely_pii(value: &str) -> bool {
    let nview = NormalizedView::build(value);
    let cand = scan_candidates(&nview.normalized);
    if !cand.any() {
        return false;
    }
    !collect_strict_redactions(&nview.normalized, &cand).is_empty()
}

/// True when `value` contains an ordinary email address. Kept separate from
/// [`has_likely_pii`] because scanner-built identifiers may legitimately
/// contain email-like `@` segments.
pub fn has_likely_email(value: &str) -> bool {
    // Cheap gate: every email requires an `@`. Skip compiling the regex when
    // the byte is absent (the common namespace/key case).
    if !value.as_bytes().contains(&b'@') {
        return false;
    }
    EMAIL_RE.is_match(value)
}

// ---------- Match collection ----------

#[derive(Debug)]
struct Hit {
    start: usize, // byte offset in NORMALIZED text
    end: usize,
    token: &'static str,
}

fn collect_redactions(norm: &str, cand: &Candidates) -> Vec<Hit> {
    collect_redactions_inner(norm, cand, true)
}

/// Variant of [`collect_redactions`] that omits bare-numeric patterns
/// whose only signal is a digit-run shape: credit card via Luhn, bare
/// CPF, bare CNPJ, NANP phones (separators optional, so any 10-11 digit
/// run starting `[2-9]`/`1[2-9]` matches), and E.164 phones (literal `+`
/// the only signal). Used for boundary checks like [`has_likely_pii`]
/// where rejection on such a hit alone would have too many false
/// positives on scanner-built identifiers (WhatsApp group JIDs
/// `<phone>-<unix>@g.us`, timestamps, padded counters).
fn collect_strict_redactions(norm: &str, cand: &Candidates) -> Vec<Hit> {
    collect_redactions_inner(norm, cand, false)
}

/// Run only the precise regexes whose class was flagged by [`scan_candidates`].
/// Priority order (and therefore overlap-resolution) is byte-identical to the
/// unconditional version; the `if cand.*` guards only decide whether each class
/// runs, so a flagged class produces exactly the hits it always did.
fn collect_redactions_inner(norm: &str, cand: &Candidates, include_bare_numeric: bool) -> Vec<Hit> {
    let mut hits: Vec<Hit> = Vec::new();

    // Priority order: most specific / highest-confidence first.
    if cand.cpf_fmt {
        push_checksum(&mut hits, norm, &CPF_FMT_RE, PII_CPF, |s| {
            valid_cpf(digits(s).as_slice())
        });
    }
    if cand.cnpj_fmt {
        push_checksum(&mut hits, norm, &CNPJ_FMT_RE, PII_CNPJ, |s| {
            valid_cnpj(digits(s).as_slice())
        });
    }
    if cand.cuit {
        push_checksum(&mut hits, norm, &CUIT_RE, PII_CUIT, |s| {
            valid_cuit(digits(s).as_slice())
        });
    }

    // IBAN before credit card: CC can match an IBAN tail of all digits.
    if cand.iban {
        push_checksum(&mut hits, norm, &IBAN_RE, PII_IBAN, valid_iban);
    }

    if include_bare_numeric {
        // Credit card before bare CPF/CNPJ to avoid catching a 13-19 digit run as CPF/CNPJ.
        if cand.cc {
            push_checksum(&mut hits, norm, &CC_RE, PII_CC, valid_luhn);
        }
        if cand.cnpj_bare {
            push_checksum(&mut hits, norm, &CNPJ_BARE_RE, PII_CNPJ, |s| {
                valid_cnpj(digits(s).as_slice())
            });
        }
        if cand.cpf_bare {
            push_checksum(&mut hits, norm, &CPF_BARE_RE, PII_CPF, |s| {
                valid_cpf(digits(s).as_slice())
            });
        }
    }

    if cand.aadhaar_fmt {
        push_checksum(&mut hits, norm, &AADHAAR_FMT_RE, PII_AADHAAR, |s| {
            valid_verhoeff(digits(s).as_slice())
        });
    }
    // Keyword-gated Aadhaar redacts only the captured 12-digit group.
    if cand.aadhaar_kw {
        push_captured(&mut hits, norm, &AADHAAR_KW_RE, PII_AADHAAR, |digits_str| {
            valid_verhoeff(digits(digits_str).as_slice())
        });
    }

    if cand.dni {
        push_checksum(&mut hits, norm, &DNI_RE, PII_DNI, valid_dni_es);
    }
    if cand.nie {
        push_checksum(&mut hits, norm, &NIE_RE, PII_DNI, valid_nie_es);
    }
    if cand.nino {
        push_checksum(&mut hits, norm, &NINO_RE, PII_NINO, valid_nino);
    }
    if cand.ssn {
        push_checksum(&mut hits, norm, &SSN_RE, PII_SSN, valid_ssn);
    }
    if cand.rrn {
        push_simple(&mut hits, norm, &RRN_RE, PII_RRN);
    }
    if cand.rfc {
        push_simple(&mut hits, norm, &RFC_RE, PII_RFC);
    }
    if cand.pan_in {
        push_simple(&mut hits, norm, &PAN_IN_RE, PII_PAN_IN);
    }

    if include_bare_numeric {
        // Phones: E.164 first (more specific), then NANP. Both are bare-numeric
        // shapes — NANP allows optional separators (`\b\d{10,11}\b` matches as
        // `XXX-XXX-XXXX`), and E.164 keys on a literal `+` with no further gate.
        // Strict callers (boundary checks like `has_likely_pii`) exclude these
        // so scanner-built namespace/key values (WhatsApp JIDs
        // `<phone>-<unix>@g.us`, telegram numeric peer IDs) don't get rejected.
        if cand.phone_e164 {
            push_simple(&mut hits, norm, &PHONE_E164_RE, PII_PHONE);
        }
        if cand.phone_nanp {
            push_simple(&mut hits, norm, &PHONE_NANP_RE, PII_PHONE);
        }
    }

    // My Number — captured digit group only, keyword remains visible.
    if cand.mynumber {
        push_captured(&mut hits, norm, &MYNUM_RE, PII_MYNUM, |_| true);
    }

    dedupe_overlaps(&mut hits);
    log::debug!(
        "[pii] collect_redactions strict={} hits={}",
        !include_bare_numeric,
        hits.len()
    );
    hits
}

fn push_simple(hits: &mut Vec<Hit>, norm: &str, re: &Regex, token: &'static str) {
    for m in re.find_iter(norm) {
        hits.push(Hit {
            start: m.start(),
            end: m.end(),
            token,
        });
    }
}

fn push_checksum(
    hits: &mut Vec<Hit>,
    norm: &str,
    re: &Regex,
    token: &'static str,
    ok: impl Fn(&str) -> bool,
) {
    for m in re.find_iter(norm) {
        if ok(m.as_str()) {
            hits.push(Hit {
                start: m.start(),
                end: m.end(),
                token,
            });
        }
    }
}

fn push_captured(
    hits: &mut Vec<Hit>,
    norm: &str,
    re: &Regex,
    token: &'static str,
    ok: impl Fn(&str) -> bool,
) {
    for caps in re.captures_iter(norm) {
        let Some(group) = caps.get(1) else { continue };
        if ok(group.as_str()) {
            hits.push(Hit {
                start: group.start(),
                end: group.end(),
                token,
            });
        }
    }
}

// Sort by start asc, length desc. Then walk in order, dropping any hit whose
// range overlaps a kept hit. Result: earlier + longer wins; no double-redact.
fn dedupe_overlaps(hits: &mut Vec<Hit>) {
    hits.sort_by(|a, b| {
        a.start
            .cmp(&b.start)
            .then((b.end - b.start).cmp(&(a.end - a.start)))
    });
    let mut kept: Vec<Hit> = Vec::with_capacity(hits.len());
    for h in hits.drain(..) {
        let overlaps = kept.last().is_some_and(|k| h.start < k.end);
        if !overlaps {
            kept.push(h);
        }
    }
    *hits = kept;
}

// Splice redactions (whose indices reference NORMALIZED text) back into the
// ORIGINAL text via NormalizedView's byte-offset mapping. This preserves
// non-PII original bytes verbatim (including fullwidth glyphs the user
// intentionally typed) while still scrubbing detected PII.
fn splice_redactions(
    original: &str,
    nview: &NormalizedView,
    hits: Vec<Hit>,
    report: &mut SanitizationReport,
) -> Sanitized<String> {
    if hits.is_empty() {
        return Sanitized {
            value: original.to_string(),
            report: *report,
        };
    }
    let mut out = String::with_capacity(original.len());
    let mut cursor = 0;
    for h in &hits {
        let start_orig = nview.norm_to_orig(h.start);
        let end_orig = nview.norm_to_orig(h.end);
        if start_orig < cursor || start_orig > original.len() || end_orig > original.len() {
            continue;
        }
        out.push_str(&original[cursor..start_orig]);
        out.push_str(h.token);
        cursor = end_orig;
    }
    out.push_str(&original[cursor..]);
    report.pii_redactions += hits.len();
    Sanitized {
        value: out,
        report: *report,
    }
}

// ---------- Unicode normalization for matching ----------
//
// A pre-pass that defeats fullwidth-digit and zero-width-char bypasses while
// keeping a byte map back to the original string, so matches found on the
// normalized view can be spliced onto the exact original bytes.

struct NormalizedView {
    normalized: String,
    // For each byte offset i in `normalized`, `byte_map[i]` is the byte offset
    // in the original string where the corresponding char *starts*.
    // The last entry maps the normalized length to the original length, so
    // `norm_to_orig(normalized.len())` is well-defined.
    byte_map: Vec<usize>,
}

impl NormalizedView {
    fn build(original: &str) -> Self {
        let mut normalized = String::with_capacity(original.len());
        let mut byte_map: Vec<usize> = Vec::with_capacity(original.len() + 1);
        for (idx, ch) in original.char_indices() {
            if is_zero_width(ch) {
                continue;
            }
            let mapped = fold_char(ch);
            let start = normalized.len();
            normalized.push(mapped);
            // One byte_map entry per byte of the normalized char.
            let added = normalized.len() - start;
            for _ in 0..added {
                byte_map.push(idx);
            }
        }
        byte_map.push(original.len());
        Self {
            normalized,
            byte_map,
        }
    }

    fn norm_to_orig(&self, norm_byte: usize) -> usize {
        if norm_byte >= self.byte_map.len() {
            return *self.byte_map.last().unwrap_or(&0);
        }
        self.byte_map[norm_byte]
    }
}

fn is_zero_width(c: char) -> bool {
    matches!(
        c,
        '\u{200B}'
            | '\u{200C}'
            | '\u{200D}'
            | '\u{200E}'
            | '\u{200F}'
            | '\u{2060}'
            | '\u{180E}'
            | '\u{FEFF}'
    )
}

fn fold_char(c: char) -> char {
    match c {
        // Fullwidth digits 0-9
        '\u{FF10}'..='\u{FF19}' => char::from_u32(c as u32 - 0xFF10 + 0x30).unwrap_or(c),
        // Arabic-Indic digits ٠-٩
        '\u{0660}'..='\u{0669}' => char::from_u32(c as u32 - 0x0660 + 0x30).unwrap_or(c),
        // Eastern Arabic-Indic digits ۰-۹
        '\u{06F0}'..='\u{06F9}' => char::from_u32(c as u32 - 0x06F0 + 0x30).unwrap_or(c),
        // Common fullwidth punctuation we care about for PII formats
        '\u{FF0D}' => '-',
        '\u{FF0E}' => '.',
        '\u{FF0F}' => '/',
        '\u{FF1A}' => ':',
        '\u{2010}'..='\u{2015}' => '-', // various unicode hyphens/dashes
        '\u{2212}' => '-',              // minus sign
        other => other,
    }
}

// ---------- Byte-oriented candidate pre-filter ----------
//
// Replaces the always-resident combined `RegexSet` (one shared NFA plus a
// per-thread lazy-DFA cache in *every* process/thread) with a single cheap pass
// over the raw bytes. The scan derives per-class candidate flags from a handful
// of structural signals — digit-run lengths, punctuation presence, uppercase /
// alpha presence, `+`, and case-insensitive keyword probes (including the
// non-Latin Aadhaar and My-Number keywords). Each flag then decides whether that
// class's precise validation regex is worth compiling and running; the precise
// `Regex`es stay `LazyLock`, so a class that never sees a candidate is never
// compiled at all. At 100–1000 concurrent agents that turns "combined NFA + N
// thread-local DFA caches resident forever" into "only the regexes a workload
// actually needs, compiled on first hit".
//
// Correctness: every flag is a NECESSARY CONDITION of the class's *precise*
// regex, so a flag can only over-fire (harmless — the precise regex then simply
// fails to match), never under-fire on real PII. Consequently, whenever a
// precise pattern would have matched without the pre-filter, its flag is set and
// it still runs — output is unchanged. The union of the flags is a superset of
// the legacy `SCREEN` set (pinned by `prefilter_is_superset_of_legacy_screen`).
// The NANP phone class gates on the *screen*-entry necessary condition — an
// internal `digit sep digit` separator OR a `\d{11,}` run (the old SCREEN reached
// `PHONE_NANP_RE` through both) — faithfully preserving the documented "a bare
// 10-digit NANP run is never reached" behavior while still redacting a bare
// `1`+10-digit country-code number — see
// `redact_pii_does_not_reach_bare_10_digit_nanp_today`.

/// Per-class candidate flags produced by [`scan_candidates`]. A set flag means
/// "run this class's precise regex"; an unset flag means the class cannot
/// possibly match, so its regex is skipped (and never compiled).
#[derive(Default, Clone, Copy)]
struct Candidates {
    cpf_fmt: bool,
    cnpj_fmt: bool,
    cuit: bool,
    iban: bool,
    cc: bool,
    cnpj_bare: bool,
    cpf_bare: bool,
    aadhaar_fmt: bool,
    aadhaar_kw: bool,
    dni: bool,
    nie: bool,
    nino: bool,
    ssn: bool,
    rrn: bool,
    rfc: bool,
    pan_in: bool,
    phone_e164: bool,
    phone_nanp: bool,
    mynumber: bool,
}

impl Candidates {
    /// True if any class is a candidate — i.e. the text is worth a precise pass.
    fn any(&self) -> bool {
        self.cpf_fmt
            || self.cnpj_fmt
            || self.cuit
            || self.iban
            || self.cc
            || self.cnpj_bare
            || self.cpf_bare
            || self.aadhaar_fmt
            || self.aadhaar_kw
            || self.dni
            || self.nie
            || self.nino
            || self.ssn
            || self.rrn
            || self.rfc
            || self.pan_in
            || self.phone_e164
            || self.phone_nanp
            || self.mynumber
    }
}

/// Case-insensitive (ASCII-only case folding) substring test over raw bytes.
/// Non-ASCII bytes compare exactly, so this also serves as an exact matcher for
/// the multibyte Devanagari / Japanese keyword needles.
fn contains_ci(hay: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() {
        return true;
    }
    if hay.len() < needle.len() {
        return false;
    }
    hay.windows(needle.len())
        .any(|w| w.iter().zip(needle).all(|(a, b)| a.eq_ignore_ascii_case(b)))
}

/// Aadhaar keyword needles — ASCII forms plus Devanagari `आधार`.
const AADHAAR_KEYWORDS: &[&[u8]] = &[b"aadhaar", b"aadhar", b"uidai", b"uid", "आधार".as_bytes()];
/// My-Number Japanese keyword needles. The English `My\s?Number` variant is
/// handled separately (see `scan_candidates`) so any `\s` separator between the
/// two words is recognised, not just a literal space.
const MYNUMBER_JP_KEYWORDS: &[&[u8]] = &["マイナンバー".as_bytes(), "個人番号".as_bytes()];

/// Single linear pass over the bytes deriving every per-class candidate flag.
///
/// Only ASCII structural bytes carry signal here; multibyte UTF-8 lead /
/// continuation bytes are all `>= 0x80`, so scanning `as_bytes()` for ASCII
/// digits/punctuation/letters is boundary-safe. Keyword probes run over the
/// same byte slice so the non-Latin needles match verbatim.
fn scan_candidates(text: &str) -> Candidates {
    let bytes = text.as_bytes();

    let mut total_digits: usize = 0;
    let mut max_digit_run: usize = 0;
    let mut cur_run: usize = 0;
    let mut has_dot = false;
    let mut has_dash = false;
    let mut has_slash = false;
    // Any ASCII whitespace separator (space, tab, newline, CR, form feed,
    // vertical tab). The precise Aadhaar pattern separates its groups with
    // `[\s-]`, which matches the whole `\s` class — so gating on space/tab
    // alone would under-fire on newline-separated Aadhaar (a real PII drop).
    let mut has_ws = false;
    let mut has_upper = false;
    let mut has_alpha = false;
    let mut has_xyz = false;
    let mut has_plus = false;
    // NANP-style "separated group" signal: some `[digit or ')'] [sep] [digit]`
    // window exists (sep ∈ space/tab/./-). This is the necessary condition of
    // the old SCREEN NANP entry, which required internal separators — keeping
    // bare separator-less 10-digit runs out of the phone path.
    let mut nanp_sep = false;

    for (i, &b) in bytes.iter().enumerate() {
        if b.is_ascii_digit() {
            total_digits += 1;
            cur_run += 1;
            if cur_run > max_digit_run {
                max_digit_run = cur_run;
            }
        } else {
            cur_run = 0;
            match b {
                b'.' => has_dot = true,
                b'-' => has_dash = true,
                b'/' => has_slash = true,
                b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c => has_ws = true,
                b'+' => has_plus = true,
                b'A'..=b'Z' => {
                    has_upper = true;
                    has_alpha = true;
                    if matches!(b, b'X' | b'Y' | b'Z') {
                        has_xyz = true;
                    }
                }
                b'a'..=b'z' => {
                    has_alpha = true;
                    if matches!(b, b'x' | b'y' | b'z') {
                        has_xyz = true;
                    }
                }
                _ => {}
            }
        }

        if matches!(b, b' ' | b'\t' | b'.' | b'-') && i > 0 && i + 1 < bytes.len() {
            let prev = bytes[i - 1];
            let next = bytes[i + 1];
            if (prev.is_ascii_digit() || prev == b')') && next.is_ascii_digit() {
                nanp_sep = true;
            }
        }
    }

    let has_digit = total_digits > 0;
    let aadhaar_kw = AADHAAR_KEYWORDS.iter().any(|kw| contains_ci(bytes, kw));
    // English `My\s?Number` accepts any single `\s` between the words, so a tab-
    // or newline-separated keyword (`My\tNumber`) must still flag. Requiring both
    // `my` and `number` substrings is a necessary condition of the precise regex
    // and covers every whitespace variant; it may over-fire (harmless — the
    // precise `MYNUM_RE` re-checks the separator and the trailing 12 digits).
    let mynumber = MYNUMBER_JP_KEYWORDS.iter().any(|kw| contains_ci(bytes, kw))
        || (contains_ci(bytes, b"my") && contains_ci(bytes, b"number"));

    let cand = Candidates {
        // Formatted CPF `\d{3}\.\d{3}\.\d{3}-\d{2}` — needs digits, `.`, `-`.
        cpf_fmt: has_digit && has_dot && has_dash,
        // Formatted CNPJ `\d{2}\.\d{3}\.\d{3}/\d{4}-\d{2}` — adds `/`.
        cnpj_fmt: has_digit && has_dot && has_slash && has_dash,
        // CUIT `\d{2}-\d{8}-\d` — needs digits and `-`.
        cuit: has_digit && has_dash,
        // IBAN `[A-Z]{2}\d{2}…` — case-sensitive uppercase letters and digits.
        iban: has_upper && has_digit,
        // Credit card `(?:\d[\s\-]?){13,19}` — at least 13 digits total.
        cc: total_digits >= 13,
        // Bare CNPJ `\d{14}` — a 14-long digit run.
        cnpj_bare: max_digit_run >= 14,
        // Bare CPF `\d{11}` — an 11-long digit run.
        cpf_bare: max_digit_run >= 11,
        // Formatted Aadhaar `\d{4}[\s-]\d{4}[\s-]\d{4}` — 12 digits + a `\s`/dash
        // separator (any ASCII whitespace, matching the precise `[\s-]` class).
        aadhaar_fmt: total_digits >= 12 && (has_ws || has_dash),
        // Keyword-gated Aadhaar — keyword suffices (precise regex checks digits).
        aadhaar_kw,
        // Spain DNI `\d{8}[A-Z]` — 8-run plus a letter.
        dni: max_digit_run >= 8 && has_alpha,
        // Spain NIE `[XYZ]\d{7}[A-Z]` — X/Y/Z, 7-run, letter.
        nie: has_xyz && max_digit_run >= 7 && has_alpha,
        // UK NINO `[A-Z]{2}\d{6}[A-D]` — letters and a 6-run.
        nino: max_digit_run >= 6 && has_alpha,
        // US SSN `\d{3}-\d{2}-\d{4}` — digits and `-`.
        ssn: has_digit && has_dash,
        // Korea RRN `\d{6}-[1-4]\d{6}` — a 6-run and `-`.
        rrn: max_digit_run >= 6 && has_dash,
        // Mexico RFC `[A-ZÑ&]{3,4}\d{6}[A-Z0-9]{3}` — a 6-run (leading class may
        // be all non-ASCII `Ñ`, so gate on the digit run alone, not on letters).
        rfc: max_digit_run >= 6,
        // India PAN `[A-Z]{5}\d{4}[A-Z]` — letters and a 4-run.
        pan_in: max_digit_run >= 4 && has_alpha,
        // E.164 `\+\d{7,15}` — a `+` and a 7+ digit run.
        phone_e164: has_plus && max_digit_run >= 7,
        // NANP — screen-entry necessary condition. The old SCREEN reached
        // `PHONE_NANP_RE` via either the separated-group pattern OR the long
        // `\d{11,}` run (which covers a bare `1`+10-digit country-code number
        // like `12025551234`). A bare 10-digit run still stays out of the phone
        // path (no internal separator, run length 10 < 11).
        phone_nanp: nanp_sep || max_digit_run >= 11,
        // My Number — keyword suffices (precise regex checks the 12 digits).
        mynumber,
    };

    log::trace!(
        "[pii] scan_candidates bytes={} digits={} max_run={} nanp_sep={} any={}",
        bytes.len(),
        total_digits,
        max_digit_run,
        nanp_sep,
        cand.any()
    );

    cand
}

// ---------- Checksum and structural validators for PII candidates ----------

fn digits(s: &str) -> Vec<u32> {
    s.chars()
        .filter(|c| c.is_ascii_digit())
        .map(|c| c.to_digit(10).expect("ascii digit"))
        .collect()
}

fn valid_cpf(d: &[u32]) -> bool {
    if d.len() != 11 || d.iter().all(|x| *x == d[0]) {
        return false;
    }
    let s1: u32 = (0..9).map(|i| d[i] * (10 - i as u32)).sum();
    let dv1 = (s1 * 10) % 11 % 10;
    if dv1 != d[9] {
        return false;
    }
    let s2: u32 = (0..10).map(|i| d[i] * (11 - i as u32)).sum();
    let dv2 = (s2 * 10) % 11 % 10;
    dv2 == d[10]
}

fn valid_cnpj(d: &[u32]) -> bool {
    if d.len() != 14 || d.iter().all(|x| *x == d[0]) {
        return false;
    }
    let w1: [u32; 12] = [5, 4, 3, 2, 9, 8, 7, 6, 5, 4, 3, 2];
    let s1: u32 = (0..12).map(|i| d[i] * w1[i]).sum();
    let r1 = s1 % 11;
    let dv1 = if r1 < 2 { 0 } else { 11 - r1 };
    if dv1 != d[12] {
        return false;
    }
    let w2: [u32; 13] = [6, 5, 4, 3, 2, 9, 8, 7, 6, 5, 4, 3, 2];
    let s2: u32 = (0..13).map(|i| d[i] * w2[i]).sum();
    let r2 = s2 % 11;
    let dv2 = if r2 < 2 { 0 } else { 11 - r2 };
    dv2 == d[13]
}

fn valid_cuit(d: &[u32]) -> bool {
    if d.len() != 11 {
        return false;
    }
    let w: [u32; 10] = [5, 4, 3, 2, 7, 6, 5, 4, 3, 2];
    let s: u32 = (0..10).map(|i| d[i] * w[i]).sum();
    let r = s % 11;
    let dv = match r {
        0 => 0,
        1 => return false,
        _ => 11 - r,
    };
    dv == d[10]
}

// Luhn — used for credit-card validation.
fn valid_luhn(s: &str) -> bool {
    let d = digits(s);
    if d.len() < 13 || d.len() > 19 {
        return false;
    }
    let mut sum = 0u32;
    let mut alt = false;
    for x in d.iter().rev() {
        let v = if alt {
            let doubled = x * 2;
            if doubled > 9 {
                doubled - 9
            } else {
                doubled
            }
        } else {
            *x
        };
        sum += v;
        alt = !alt;
    }
    sum.is_multiple_of(10)
}

// IBAN mod-97. Steps: strip spaces, move first 4 chars to end, expand letters
// (A=10..Z=35), divide as a big-integer mod 97, require remainder == 1.
fn valid_iban(s: &str) -> bool {
    let cleaned: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    if cleaned.len() < 15 || cleaned.len() > 34 {
        return false;
    }
    if !cleaned.chars().take(2).all(|c| c.is_ascii_alphabetic()) {
        return false;
    }
    if !cleaned[2..4].chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    let rotated: String = cleaned[4..].chars().chain(cleaned[..4].chars()).collect();
    let mut remainder: u64 = 0;
    for c in rotated.chars() {
        let chunk = if let Some(d) = c.to_digit(10) {
            d as u64
        } else if c.is_ascii_alphabetic() {
            (c.to_ascii_uppercase() as u64) - ('A' as u64) + 10
        } else {
            return false;
        };
        // Expand into the running remainder digit-by-digit so we never need
        // u128. Each letter contributes 2 decimal digits.
        if chunk >= 10 {
            remainder = (remainder * 100 + chunk) % 97;
        } else {
            remainder = (remainder * 10 + chunk) % 97;
        }
    }
    remainder == 1
}

// Verhoeff — used for Aadhaar.
const VERHOEFF_D: [[u8; 10]; 10] = [
    [0, 1, 2, 3, 4, 5, 6, 7, 8, 9],
    [1, 2, 3, 4, 0, 6, 7, 8, 9, 5],
    [2, 3, 4, 0, 1, 7, 8, 9, 5, 6],
    [3, 4, 0, 1, 2, 8, 9, 5, 6, 7],
    [4, 0, 1, 2, 3, 9, 5, 6, 7, 8],
    [5, 9, 8, 7, 6, 0, 4, 3, 2, 1],
    [6, 5, 9, 8, 7, 1, 0, 4, 3, 2],
    [7, 6, 5, 9, 8, 2, 1, 0, 4, 3],
    [8, 7, 6, 5, 9, 3, 2, 1, 0, 4],
    [9, 8, 7, 6, 5, 4, 3, 2, 1, 0],
];
const VERHOEFF_P: [[u8; 10]; 8] = [
    [0, 1, 2, 3, 4, 5, 6, 7, 8, 9],
    [1, 5, 7, 6, 2, 8, 3, 0, 9, 4],
    [5, 8, 0, 3, 7, 9, 6, 1, 4, 2],
    [8, 9, 1, 6, 0, 4, 3, 5, 2, 7],
    [9, 4, 5, 3, 1, 2, 6, 8, 7, 0],
    [4, 2, 8, 6, 5, 7, 3, 9, 0, 1],
    [2, 7, 9, 3, 8, 0, 6, 4, 1, 5],
    [7, 0, 4, 6, 9, 1, 3, 2, 5, 8],
];

fn valid_verhoeff(d: &[u32]) -> bool {
    if d.len() != 12 {
        return false;
    }
    // Aadhaar can't start with 0 or 1.
    if d[0] < 2 {
        return false;
    }
    let mut c: u8 = 0;
    for (i, digit) in d.iter().rev().enumerate() {
        c = VERHOEFF_D[c as usize][VERHOEFF_P[i % 8][*digit as usize] as usize];
    }
    c == 0
}

// US SSN reserved/invalid ranges per SSA.
fn valid_ssn(s: &str) -> bool {
    let d = digits(s);
    if d.len() != 9 {
        return false;
    }
    let area = d[0] * 100 + d[1] * 10 + d[2];
    let group = d[3] * 10 + d[4];
    let serial = d[5] * 1000 + d[6] * 100 + d[7] * 10 + d[8];
    if area == 0 || area == 666 || area >= 900 {
        return false;
    }
    if group == 0 || serial == 0 {
        return false;
    }
    true
}

// Spain DNI check letter — 8 digits mod 23 indexes into a fixed letter table.
const DNI_LETTERS: &[u8; 23] = b"TRWAGMYFPDXBNJZSQVHLCKE";

fn valid_dni_es(s: &str) -> bool {
    let upper = s.to_ascii_uppercase();
    let bytes = upper.as_bytes();
    if bytes.len() != 9 {
        return false;
    }
    let num_str = &upper[..8];
    let letter = bytes[8];
    let Ok(num) = num_str.parse::<u32>() else {
        return false;
    };
    DNI_LETTERS[(num % 23) as usize] == letter
}

fn valid_nie_es(s: &str) -> bool {
    let upper = s.to_ascii_uppercase();
    let bytes = upper.as_bytes();
    if bytes.len() != 9 {
        return false;
    }
    let prefix = match bytes[0] {
        b'X' => 0u32,
        b'Y' => 1,
        b'Z' => 2,
        _ => return false,
    };
    let Ok(rest) = std::str::from_utf8(&bytes[1..8]) else {
        return false;
    };
    let Ok(num) = rest.parse::<u32>() else {
        return false;
    };
    let composed = prefix * 10_000_000 + num;
    DNI_LETTERS[(composed % 23) as usize] == bytes[8]
}

// UK NINO reserved-prefix blacklist.
fn valid_nino(s: &str) -> bool {
    let upper = s.to_ascii_uppercase();
    let bytes = upper.as_bytes();
    if bytes.len() != 9 {
        return false;
    }
    // First char cannot be D F I Q U V; second cannot be D F I O Q U V.
    let bad_first = b"DFIQUV";
    let bad_second = b"DFIOQUV";
    if bad_first.contains(&bytes[0]) || bad_second.contains(&bytes[1]) {
        return false;
    }
    // Reserved two-letter prefixes.
    let reserved = ["BG", "GB", "KN", "NK", "NT", "TN", "ZZ"];
    let prefix = &upper[..2];
    if reserved.contains(&prefix) {
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Assembled rather than written out so a repository secret scanner does
    /// not read the fixture as a real key block.
    fn private_key_fixture(kind: &str, body: &str) -> String {
        format!("-----BEGIN {kind}-----\n{body}\n-----END {kind}-----")
    }

    #[test]
    fn sanitize_text_redacts_bearer_and_openai_key() {
        let input = "Authorization: Bearer abcdefghijklmnop and sk-1234567890123456789012345";
        let sanitized = sanitize_text(input);
        assert!(sanitized.value.contains("Bearer [REDACTED]"));
        assert!(!sanitized.value.contains("sk-1234567890123456789012345"));
        assert!(sanitized.report.text_redactions >= 2);
    }

    #[test]
    fn sanitize_text_blocks_private_key_blocks() {
        let input = private_key_fixture("PRIVATE KEY", "abc");
        let sanitized = sanitize_text(&input);
        assert!(sanitized.value.contains(REDACTED_PRIVATE_KEY));
        assert!(sanitized.report.blocked_secret_hits >= 1);
    }

    #[test]
    fn sanitize_json_redacts_sensitive_keys_and_nested_strings() {
        let input = json!({
            "token": "abc123",
            "nested": { "notes": "Bearer supersecretvalue", "ok": "hello" },
            "arr": ["sk-1234567890123456789012345", "safe"]
        });
        let sanitized = sanitize_json(&input);
        assert_eq!(sanitized.value["token"], json!(REDACTED_SECRET));
        assert_eq!(sanitized.value["nested"]["ok"], json!("hello"));
        assert!(sanitized.value["nested"]["notes"]
            .as_str()
            .unwrap_or_default()
            .contains("[REDACTED]"));
        assert!(sanitized.report.key_redactions >= 1);
        assert!(sanitized.report.text_redactions >= 2);
    }

    #[test]
    fn sanitize_json_redacts_common_sensitive_key_variants() {
        let input = json!({
            "db_password": "p@ss", "secret_key": "abc123",
            "api_secret": "def456", "monkey": "banana"
        });
        let sanitized = sanitize_json(&input);
        assert_eq!(sanitized.value["db_password"], json!(REDACTED_SECRET));
        assert_eq!(sanitized.value["secret_key"], json!(REDACTED_SECRET));
        assert_eq!(sanitized.value["api_secret"], json!(REDACTED_SECRET));
        assert_eq!(sanitized.value["monkey"], json!(REDACTED_SECRET));
        assert!(sanitized.report.key_redactions >= 4);
    }

    #[test]
    fn has_likely_secret_detects_common_patterns() {
        assert!(has_likely_secret("api_key=abc123"));
        assert!(has_likely_secret("Bearer abcdefghijklmnopqrstuvwxyz"));
        let slack_token = format!("{}{}-1234567890-abcdef-ghijklmnop", "xo", "xb");
        assert!(has_likely_secret(&slack_token));
        assert!(has_likely_secret("glpat-aaaaaaaaaaaaaaaaaaaa"));
        assert!(has_likely_secret("SG.aaaaaaaaaaaaaaaa.bbbbbbbbbbbbbbbb"));
        assert!(!has_likely_secret("I prefer rust"));
    }

    #[test]
    fn sanitize_text_redacts_more_provider_secrets() {
        let input = "auth=Basic QWxhZGRpbjpvcGVuIHNlc2FtZQ== \
                     stripe=sk_live_12345678901234567890 npm=npm_abcdefghijklmnopqrstuvwxyz";
        let sanitized = sanitize_text(input);
        assert!(!sanitized.value.contains("sk_live_12345678901234567890"));
        assert!(!sanitized.value.contains("npm_abcdefghijklmnopqrstuvwxyz"));
        assert!(sanitized.value.contains("[REDACTED]"));
        assert!(sanitized.report.text_redactions >= 2);
    }

    #[test]
    fn sanitize_text_redacts_oauth_url_style_params() {
        let input = "https://example.com/callback\
            ?access_token=abcd1234&refresh_token=efgh5678&id_token=jwt";
        let sanitized = sanitize_text(input);
        assert!(!sanitized.value.contains("abcd1234"));
        assert!(!sanitized.value.contains("efgh5678"));
        assert!(!sanitized.value.contains("id_token=jwt"));
        assert!(sanitized.report.text_redactions >= 3);
    }

    #[test]
    fn sanitize_text_redacts_multiline_private_key_blocks() {
        let key_kind = format!("{} PRIVATE KEY", "OPENSSH");
        let input = format!(
            "BEGIN\n{}\nEND",
            private_key_fixture(&key_kind, "line1\nline2")
        );
        let sanitized = sanitize_text(&input);
        assert!(!sanitized.value.contains(&key_kind));
        assert!(sanitized.value.contains(REDACTED_PRIVATE_KEY));
        assert!(sanitized.report.blocked_secret_hits >= 1);
    }

    #[test]
    fn sanitize_text_also_redacts_pii_after_secrets() {
        let input = "Token sk-abcdefghijklmnopqrstuvwxyz; CPF 111.444.777-35; phone +15551234567";
        let sanitized = sanitize_text(input);
        assert!(!sanitized.value.contains("sk-abcdefghijklmnopqrstuvwxyz"));
        assert!(!sanitized.value.contains("111.444.777-35"));
        assert!(!sanitized.value.contains("+15551234567"));
        assert!(sanitized.value.contains("[REDACTED_PII_CPF]"));
        assert!(sanitized.value.contains("[REDACTED_PII_PHONE]"));
        assert!(sanitized.report.text_redactions >= 1);
        assert_eq!(sanitized.report.pii_redactions, 2);
    }

    #[test]
    fn sanitize_json_propagates_pii_redaction_into_nested_strings() {
        let input = json!({
            "note": "Cliente RFC VECJ880326XK4 confirmado",
            "meta": { "cuit": "20-11111111-2" }
        });
        let sanitized = sanitize_json(&input);
        assert!(sanitized.value["note"]
            .as_str()
            .unwrap_or_default()
            .contains("[REDACTED_PII_RFC]"));
        assert!(sanitized.value["meta"]["cuit"]
            .as_str()
            .unwrap_or_default()
            .contains("[REDACTED_PII_CUIT]"));
        assert!(sanitized.report.pii_redactions >= 2);
    }

    #[test]
    fn sanitize_json_redacts_values_beyond_max_depth() {
        let mut nested = json!("leaf");
        for _ in 0..(MAX_JSON_SANITIZE_DEPTH + 2) {
            nested = json!({ "nested": nested });
        }
        let sanitized = sanitize_json(&nested);
        assert!(sanitized.report.depth_redactions >= 1);
        assert!(sanitized
            .value
            .to_string()
            .contains(&format!("\"{REDACTED_SECRET}\"")));
    }

    #[test]
    fn has_likely_pii_strict_boundary_flags_formatted_national_ids() {
        // The write-rejection boundary is the *strict* set: formatted national IDs
        // only. Bare-numeric / phone-shaped runs and email are excluded (too many
        // false positives against scanner-built identifiers); they are still
        // scrubbed by content redaction.
        assert!(has_likely_pii("ssn 123-45-6789"));
        assert!(has_likely_pii("CPF 111.444.777-35"));
        assert!(has_likely_pii("cliente RFC VECJ880326XK4"));
        assert!(!has_likely_pii("call +15551234567")); // phone: content-scrub only
        assert!(!has_likely_pii("contact alice@example.com")); // email: out of scope
        assert!(!has_likely_pii("just a normal note"));
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

    // --- CPF ---
    #[test]
    fn cpf_formatted_valid_redacted() {
        redacts("CPF: 111.444.777-35.", PII_CPF);
    }
    #[test]
    fn cpf_formatted_invalid_kept() {
        unchanged("CPF 111.444.777-99 nope");
    }
    #[test]
    fn cpf_all_same_digits_rejected() {
        unchanged("Test 111.111.111-11");
    }
    #[test]
    fn cpf_bare_valid_redacted() {
        redacts("Sem mascara 11144477735 ok", PII_CPF);
    }

    // --- CNPJ ---
    #[test]
    fn cnpj_formatted_valid_redacted() {
        redacts("CNPJ 11.222.333/0001-81", PII_CNPJ);
    }
    #[test]
    fn cnpj_bare_valid_redacted() {
        redacts("contract 11222333000181 yes", PII_CNPJ);
    }

    // --- CUIT ---
    #[test]
    fn cuit_valid_redacted() {
        redacts("CUIT 20-11111111-2", PII_CUIT);
    }
    #[test]
    fn cuit_invalid_kept() {
        unchanged("noise 20-12345678-0 noise");
    }

    // --- RFC ---
    #[test]
    fn rfc_redacted() {
        redacts("Mi RFC VECJ880326XK4 .", PII_RFC);
    }
    #[test]
    fn rfc_lowercase_redacted() {
        redacts("rfc vecj880326xk4", PII_RFC);
    }

    // --- My Number ---
    #[test]
    fn my_number_redacted_with_keyword() {
        redacts("マイナンバー: 123456789012", PII_MYNUM);
    }
    #[test]
    fn bare_12_digits_without_keyword_kept() {
        unchanged("Order 123456789012 shipped today.");
    }
    #[test]
    fn my_number_keyword_tab_separator_redacted() {
        // `My\s?Number` accepts any single `\s`; the byte prefilter must recognise a
        // tab-separated keyword, not just a literal space.
        redacts("My\tNumber 123456789012", PII_MYNUM);
    }
    #[test]
    fn my_number_keyword_newline_separator_redacted() {
        redacts("My\nNumber 123456789012", PII_MYNUM);
    }

    // --- E.164 + NANP phone ---
    #[test]
    fn e164_redacted() {
        redacts("phone +15551234567", PII_PHONE);
    }
    #[test]
    fn nanp_formatted_redacted() {
        redacts("call 415-555-0123 thanks", PII_PHONE);
    }
    #[test]
    fn nanp_with_country_code_redacted() {
        redacts("+1 (212) 555-7890", PII_PHONE);
    }
    #[test]
    fn nanp_invalid_area_code_kept() {
        unchanged("score 115-555-0123 ish");
    }
    #[test]
    fn nanp_bare_country_code_redacted() {
        // Separator-less `1`+10-digit NANP: the old SCREEN reached PHONE_NANP_RE via
        // the `\d{11,}` run; the prefilter must keep gating this through the phone
        // class (the bare-CPF checksum rejects it, so nothing else redacts it).
        redacts("12025551234", PII_PHONE);
    }

    // --- SSN ---
    #[test]
    fn ssn_valid_redacted() {
        redacts("ssn 123-45-6789", PII_SSN);
    }
    #[test]
    fn ssn_reserved_area_kept() {
        unchanged("test 666-12-3456");
    }
    #[test]
    fn ssn_zero_serial_kept() {
        unchanged("test 123-45-0000");
    }

    // --- Credit card / Luhn ---
    #[test]
    fn credit_card_visa_redacted() {
        // Visa test number with valid Luhn.
        redacts("card 4111 1111 1111 1111 thanks", PII_CC);
    }
    #[test]
    fn credit_card_amex_redacted() {
        redacts("card 378282246310005 used", PII_CC);
    }
    #[test]
    fn credit_card_invalid_luhn_kept() {
        unchanged("invoice 4111 1111 1111 1112");
    }

    // --- IBAN ---
    #[test]
    fn iban_de_redacted() {
        // Known test IBAN with valid mod-97.
        redacts("IBAN DE89370400440532013000 ok", PII_IBAN);
    }
    #[test]
    fn iban_invalid_kept() {
        unchanged("noise DE89370400440532013001 noise");
    }

    // --- Aadhaar ---
    #[test]
    fn aadhaar_formatted_verhoeff_valid_redacted() {
        // 234123412346 is a known Verhoeff-valid Aadhaar test number.
        redacts("Aadhaar 2341 2341 2346", PII_AADHAAR);
    }
    #[test]
    fn aadhaar_keyword_bare_redacted() {
        redacts("Aadhaar: 234123412346", PII_AADHAAR);
    }
    #[test]
    fn aadhaar_invalid_verhoeff_kept() {
        unchanged("Random 2341 2341 2345 nope");
    }
    #[test]
    fn aadhaar_formatted_newline_separator_redacted() {
        // AADHAAR_FMT_RE separates groups with `[\s-]`; a newline-separated Aadhaar
        // (no keyword, no dash) must still flag the formatted class in the prefilter.
        redacts("2341\n2341\n2346", PII_AADHAAR);
    }

    // --- PAN-IN ---
    #[test]
    fn pan_in_redacted() {
        redacts("PAN: ABCDE1234F", PII_PAN_IN);
    }

    // --- NINO ---
    #[test]
    fn nino_redacted() {
        redacts("NI no AB123456C", PII_NINO);
    }
    #[test]
    fn nino_reserved_prefix_kept() {
        unchanged("BG123456A");
    }

    // --- DNI / NIE ---
    #[test]
    fn dni_es_redacted() {
        redacts("DNI 12345678Z", PII_DNI);
    }
    #[test]
    fn dni_es_bad_letter_kept() {
        unchanged("ID 12345678A code");
    }
    #[test]
    fn nie_es_redacted() {
        redacts("NIE X1234567L", PII_DNI);
    }

    // --- RRN Korea ---
    #[test]
    fn rrn_kr_redacted() {
        redacts("주민번호 900101-1234567", PII_RRN);
    }
    #[test]
    fn rrn_kr_bad_gender_digit_kept() {
        unchanged("ref 900101-5234567 nope");
    }

    // --- Bypass resistance ---
    #[test]
    fn fullwidth_digits_cannot_bypass_cpf() {
        // 111.444.777-35 with fullwidth digits and punctuation.
        let input = "CPF: １１１．４４４．７７７－３５ done";
        let out = redact_pii(input);
        assert!(out.value.contains(PII_CPF), "got {out:?}");
    }

    #[test]
    fn zero_width_chars_cannot_bypass_ssn() {
        // U+200B inserted between digits.
        let input = "ssn 1\u{200B}23-4\u{200B}5-6789 done";
        let out = redact_pii(input);
        assert!(out.value.contains(PII_SSN), "got {out:?}");
    }

    #[test]
    fn arabic_indic_digits_normalize_for_phone() {
        let input = "phone +١٥٥٥١٢٣٤٥٦٧";
        let out = redact_pii(input);
        assert!(out.value.contains(PII_PHONE), "got {out:?}");
    }

    // --- Aggressive mix end-to-end ---
    #[test]
    fn aggressive_mixed_document() {
        let input = "\
    Cliente RFC VECJ880326XK4. \
    Empresa CNPJ 11.222.333/0001-81. \
    Argentino CUIT 20-11111111-2. \
    Brasileiro CPF 111.444.777-35. \
    マイナンバー: 123456789012. \
    SSN 123-45-6789. \
    Card 4111 1111 1111 1111. \
    IBAN DE89370400440532013000. \
    PAN ABCDE1234F. \
    NI AB123456C. \
    DNI 12345678Z. \
    RRN 900101-1234567. \
    Phone +15551234567.";
        let out = redact_pii(input);
        for token in [
            PII_RFC, PII_CNPJ, PII_CUIT, PII_CPF, PII_MYNUM, PII_SSN, PII_CC, PII_IBAN, PII_PAN_IN,
            PII_NINO, PII_DNI, PII_RRN, PII_PHONE,
        ] {
            assert!(
                out.value.contains(token),
                "missing {token} in: {}",
                out.value
            );
        }
        assert!(out.report.pii_redactions >= 13);
    }

    // --- has_likely_pii ---
    #[test]
    fn has_likely_pii_detects_cpf() {
        assert!(has_likely_pii("user/111.444.777-35"));
    }

    #[test]
    fn has_likely_email_detects_email_without_changing_boundary_pii() {
        assert!(has_likely_email("user/alice@example.com"));
        assert!(!has_likely_pii("user/alice@example.com"));
    }
    #[test]
    fn has_likely_pii_quiet_on_normal_text() {
        assert!(!has_likely_pii("memory/global/preferences"));
    }

    /// Regression: zero-padded millisecond-timestamp keys must NOT be
    /// flagged as PII even when the digit run happens to satisfy Luhn.
    /// `redact_pii` content scrubbing may still flag the same string —
    /// `has_likely_pii` (used for boundary rejection of internal keys)
    /// must stay strict to formatted/keyword PII only.
    #[test]
    fn has_likely_pii_ignores_bare_luhn_timestamp_keys() {
        // 18-digit padded timestamps where the digit total mod 10 == 0
        // (the Luhn-passing case that previously rejected autocomplete
        // KV writes and screen-intelligence document writes).
        for key in [
            "accepted:000001747729035001",
            "completion:000001747729035011",
            "screen_intelligence_vision-1747729035001-VSCode",
        ] {
            assert!(
                !has_likely_pii(key),
                "internal key {key:?} must not be rejected as PII"
            );
        }
    }

    /// Strict boundary check should still reject formatted PII even though
    /// it skips bare-numeric checksum patterns.
    #[test]
    fn has_likely_pii_still_blocks_formatted_secrets() {
        assert!(has_likely_pii("ssn-123-45-6789"));
        assert!(has_likely_pii("cliente-RFC-VECJ880326XK4"));
        assert!(has_likely_pii("cuit-20-11111111-2"));
    }

    /// Regression for Sentry TAURI-RUST-54T / GH #2848: scanner-built
    /// `namespace` and `key` values containing bare-numeric phone-shaped
    /// digit runs (WhatsApp group JID `<phone>-<unix>@g.us`, WhatsApp
    /// broadcast `<phone>@broadcast`, US-prefixed WhatsApp 1:1 JID,
    /// telegram numeric peer ID) must NOT be rejected by the boundary
    /// PII check. NANP matches `\d{10,11}` with optional separators —
    /// strict mode must skip it. Content scrubbing via `redact_pii`
    /// continues to redact these substrings (see
    /// `redact_pii_still_blurs_bare_phone_in_content` below).
    #[test]
    fn has_likely_pii_ignores_scanner_bare_phone_keys() {
        for key in [
            // WhatsApp group JID — chat_id = "<phone>-<unix-ts>@g.us"
            "12025551234-1543890267@g.us:2026-05-30",
            // WhatsApp broadcast list
            "12025551234@broadcast:2026-05-30",
            // WhatsApp 1:1 JID, country-coded US number (`1` + 10 digits)
            "12025551234@c.us:2026-05-30",
            // Same shape carried in the namespace
            "whatsapp-web:12025551234@c.us",
            "whatsapp-web:12025551234-1543890267@g.us",
            // Telegram numeric peer_id key
            "4123456789:2026-05-30",
        ] {
            assert!(
                !has_likely_pii(key),
                "scanner-built key {key:?} must not be rejected as PII"
            );
        }
    }

    /// Same regression but for the E.164 (`+`-prefixed) shape — iMessage
    /// posts `key = format!("{chat_id}:{day}")` where `chat_id` can be
    /// `+12025551234`. Strict mode must skip; content redaction stays.
    #[test]
    fn has_likely_pii_ignores_bare_e164_phone_keys() {
        for key in [
            "+12025551234:2026-05-30",
            "imessage:+12025551234",
            "imessage:+12025551234:2026-05-30",
        ] {
            assert!(
                !has_likely_pii(key),
                "E.164-shaped key {key:?} must not be rejected as PII"
            );
        }
    }

    /// `redact_pii` (content scrubbing path — NOT the boundary check)
    /// must still redact formatted NANP and E.164 phone numbers found
    /// inside document bodies. False positives in the content path only
    /// blur substring bytes; they do not reject the write — which is the
    /// asymmetry this PR preserves vs. the boundary check.
    ///
    /// Note: bare 10-digit NANP runs (`2025551234` with no separators)
    /// are NOT reached by `redact_pii` at all — the SCREEN fast-path
    /// requires either `\d{11,}`, a separator, or `+`, so a bare 10-digit
    /// run short-circuits as "no candidate". That pre-existed this PR; a
    /// pinning sentinel for it lives below.
    #[test]
    fn redact_pii_still_blurs_formatted_and_e164_phone_in_content() {
        let out = redact_pii("call me at 202-555-1234 or +12025551234");
        let n_phone = out.value.matches(PII_PHONE).count();
        assert!(
            n_phone >= 2,
            "redact_pii must still blur both formatted NANP and E.164 phones in content, \
                 got {n_phone} PII_PHONE token(s) in: {}",
            out.value
        );
        assert!(out.report.pii_redactions >= 2);
    }

    /// Sentinel pinning a pre-existing SCREEN limitation: a bare 10-digit
    /// NANP run (`2025551234` with no separators) is short-circuited by
    /// the `SCREEN` fast-path because no `SCREEN` regex matches a 10-digit
    /// bare run (`\d{11,}` is the closest, but it needs 11+). This is the
    /// status quo on `main` — this PR does not change it. The test exists
    /// so any future widening of `SCREEN` (e.g. to catch bare NANP) trips
    /// here as a deliberate review checkpoint, NOT a regression.
    #[test]
    fn redact_pii_does_not_reach_bare_10_digit_nanp_today() {
        let out = redact_pii("call me at 2025551234 thanks");
        assert!(
            !out.value.contains(PII_PHONE),
            "SCREEN fast-path historically skips bare 10-digit NANP — \
                 if this test fails, SCREEN was widened; revisit the boundary-check \
                 behavior in `has_likely_pii` before adjusting. Got: {}",
            out.value
        );
    }

    #[test]
    fn empty_text_is_noop() {
        unchanged("");
    }

    // --- Byte prefilter: per-class positives (incl. non-Latin) ---

    /// Devanagari Aadhaar keyword must still route into the keyword-gated Aadhaar
    /// path (the `आधार` needle lives in `AADHAAR_KEYWORDS`).
    #[test]
    fn aadhaar_devanagari_keyword_redacted() {
        redacts("आधार 234123412346", PII_AADHAAR);
    }

    /// Japanese My-Number keyword (kanji form) routes into the My-Number path.
    #[test]
    fn my_number_kanji_keyword_redacted() {
        redacts("個人番号 123456789012", PII_MYNUM);
    }

    /// `scan_candidates` flags the right class for representative per-class inputs.
    #[test]
    fn scan_flags_expected_classes() {
        assert!(scan_candidates("111.444.777-35").cpf_fmt);
        assert!(scan_candidates("11.222.333/0001-81").cnpj_fmt);
        assert!(scan_candidates("20-11111111-2").cuit);
        assert!(scan_candidates("DE89370400440532013000").iban);
        assert!(scan_candidates("4111111111111111").cc);
        assert!(scan_candidates("11222333000181").cnpj_bare);
        assert!(scan_candidates("11144477735").cpf_bare);
        assert!(scan_candidates("2341 2341 2346").aadhaar_fmt);
        assert!(scan_candidates("aadhaar 234123412346").aadhaar_kw);
        assert!(scan_candidates("आधार 234123412346").aadhaar_kw);
        assert!(scan_candidates("12345678Z").dni);
        assert!(scan_candidates("X1234567L").nie);
        assert!(scan_candidates("AB123456C").nino);
        assert!(scan_candidates("123-45-6789").ssn);
        assert!(scan_candidates("900101-1234567").rrn);
        assert!(scan_candidates("VECJ880326XK4").rfc);
        assert!(scan_candidates("ABCDE1234F").pan_in);
        assert!(scan_candidates("+15551234567").phone_e164);
        assert!(scan_candidates("415-555-0123").phone_nanp);
        assert!(scan_candidates("マイナンバー 123456789012").mynumber);
        assert!(scan_candidates("My Number 123456789012").mynumber);
    }

    /// Clean, PII-free text flags no class at all — the whole precise pass is
    /// skipped and every precise regex stays uncompiled.
    #[test]
    fn scan_clean_text_flags_nothing() {
        for clean in [
            "",
            "just some ordinary words here",
            "memory/global/preferences",
            "the quick brown fox",
            "https://example.com/path?q=1",
            "snake_case_identifier_v2",
        ] {
            let cand = scan_candidates(clean);
            assert!(!cand.any(), "clean text flagged a class: {clean:?}");
        }
    }

    /// A bare separator-less 10-digit run must NOT flag the NANP phone class — this
    /// is what preserves the documented "bare 10-digit NANP is never reached"
    /// behavior even though the precise NANP regex would otherwise match it.
    #[test]
    fn scan_bare_10_digit_run_does_not_flag_nanp() {
        assert!(!scan_candidates("call me at 2025551234 thanks").phone_nanp);
    }

    /// Parity oracle: the new byte prefilter must be a SUPERSET of the legacy
    /// `SCREEN` regex set. For every corpus input, if the old combined set would
    /// have matched the normalized text, the new per-class scan must flag at least
    /// one class — otherwise a real PII candidate would be silently dropped.
    #[test]
    fn prefilter_is_superset_of_legacy_screen() {
        use regex::RegexSet;

        // Byte-for-byte the pattern list this PR removed from `pii.rs`.
        let legacy_screen = RegexSet::new([
            r"\d{11,}",
            r"\d{3}\.\d{3}\.\d{3}-\d{2}",
            r"\d{2}\.\d{3}\.\d{3}/\d{4}-\d{2}",
            r"\d{2}-\d{8}-\d",
            r"(?i)[A-Z]{3,4}\d{6}",
            r"(?:マイナンバー|個人番号|My\s?Number)",
            r"\+\d{7}",
            r"\(?[2-9]\d{2}\)?[\s.\-]\d{3}[\s.\-]\d{4}",
            r"\d{3}-\d{2}-\d{4}",
            r"\b[A-Z]{2}\d{2}[A-Z0-9]",
            r"\d{4}[\s\-]\d{4}[\s\-]\d{4}",
            r"(?i)aadhaar|aadhar|आधार|uidai",
            r"(?i)[A-Z]{5}\d{4}[A-Z]",
            r"(?i)[A-Z]{2}\d{6}[A-D]",
            r"\b\d{8}[A-Z]\b",
            r"(?i)[XYZ]\d{7}[A-Z]",
            r"\d{6}-[1-4]\d{6}",
        ])
        .expect("legacy screen");

        let corpus = [
            // Real PII, one per class.
            "CPF: 111.444.777-35.",
            "Sem mascara 11144477735 ok",
            "CNPJ 11.222.333/0001-81",
            "contract 11222333000181 yes",
            "CUIT 20-11111111-2",
            "Mi RFC VECJ880326XK4 .",
            "マイナンバー: 123456789012",
            "個人番号 123456789012",
            "My Number 123456789012",
            // Whitespace-separator variants the precise regexes accept via `\s`.
            "My\tNumber 123456789012",
            "My\nNumber 123456789012",
            "2341\n2341\n2346",
            "12025551234",
            "phone +15551234567",
            "call 415-555-0123 thanks",
            "+1 (212) 555-7890",
            "ssn 123-45-6789",
            "card 4111 1111 1111 1111 thanks",
            "card 378282246310005 used",
            "IBAN DE89370400440532013000 ok",
            "Aadhaar 2341 2341 2346",
            "Aadhaar: 234123412346",
            "आधार 234123412346",
            "uidai 234123412346",
            "PAN: ABCDE1234F",
            "NI no AB123456C",
            "DNI 12345678Z",
            "NIE X1234567L",
            "주민번호 900101-1234567",
            // Scanner-built / borderline identifiers.
            "12025551234-1543890267@g.us:2026-05-30",
            "+12025551234:2026-05-30",
            "accepted:000001747729035001",
            "screen_intelligence_vision-1747729035001-VSCode",
            "Order 123456789012 shipped today.",
            // Clean text (screen won't match; nothing to assert but exercises path).
            "memory/global/preferences",
            "the quick brown fox jumps",
            "just some ordinary words here",
        ];

        for input in corpus {
            let nview = NormalizedView::build(input);
            if legacy_screen.is_match(&nview.normalized) {
                assert!(
                    scan_candidates(&nview.normalized).any(),
                    "legacy SCREEN matched but new prefilter flagged nothing: {input:?}"
                );
            }
        }
    }

    #[test]
    fn tax_ids_enforce_lengths_checksums_and_repetition_rules() {
        assert!(valid_cpf(&digits("529.982.247-25")));
        assert!(!valid_cpf(&digits("111.111.111-11")));
        assert!(!valid_cpf(&digits("5299822472")));
        assert!(valid_cnpj(&digits("11.222.333/0001-81")));
        assert!(!valid_cnpj(&digits("11.222.333/0001-82")));
        assert!(!valid_cnpj(&digits("00000000000000")));
        assert!(valid_cuit(&digits("20-12345678-6")));
        assert!(!valid_cuit(&digits("20-12345678-7")));
        assert!(!valid_cuit(&digits("2012345678")));
    }

    #[test]
    fn payment_checksums_reject_bad_bounds_and_checksums() {
        assert!(valid_luhn("4111 1111 1111 1111"));
        assert!(!valid_luhn("4111 1111 1111 1112"));
        assert!(!valid_luhn("7992739871"));
        assert!(valid_iban("GB82 WEST 1234 5698 7654 32"));
        assert!(!valid_iban("GB82 WEST 1234 5698 7654 33"));
        assert!(!valid_iban("GB00"));
    }

    #[test]
    fn identity_validators_cover_checksums_reserved_values_and_prefixes() {
        assert!(valid_verhoeff(&digits("234567890124")));
        assert!(!valid_verhoeff(&digits("134567890124")));
        assert!(!valid_verhoeff(&digits("234567890125")));
        assert!(valid_ssn("123-45-6789"));
        assert!(!valid_ssn("666-45-6789"));
        assert!(!valid_ssn("123-00-6789"));
        assert!(!valid_ssn("123-45-0000"));
        assert!(valid_dni_es("12345678Z"));
        assert!(!valid_dni_es("12345678A"));
        assert!(valid_nie_es("X1234567L"));
        assert!(!valid_nie_es("A1234567L"));
        assert!(valid_nino("AA123456A"));
        assert!(!valid_nino("BG123456A"));
        assert!(!valid_nino("DA123456A"));
        assert!(!valid_nino("AA12345A"));
    }
}
