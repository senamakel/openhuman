use once_cell::sync::Lazy;
use regex::Regex;

static EMAIL: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\b[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}\b").unwrap());

// Catches +1-415-555-0123, (415) 555-0123, 415-555-0123 style phone numbers.
// Uses mandatory separators / parentheses to distinguish phone patterns from
// plain numeric IDs. The `regex` crate does not support look-around, so
// boundaries are enforced structurally. Runs of unseparated digits (e.g.
// "4155550123") are intentionally not matched — indistinguishable from IDs.
//
// Pattern groups:
//   A) Paren area code: optional leading "+" and country-code prefix, then
//      "(NXX)" followed by space/dash and the 7-digit local number.
//   B) Separator area code: "+?" optional country code, then "NXX" + mandatory
//      separator, then NXX-XXXX local.
static PHONE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?x)
        (?:
            # Group A: parenthesised area code  e.g. (415) 555-0123
            # May be prefixed by +1 or 1 with a separator.
            (?:\+?\d{1,3}[\s\-.])?      # optional country code + separator
            \(\d{2,4}\)                 # (NXX) area code
            [\s\-.]                     # mandatory separator after paren
            \d{3}[\s\-.]?\d{4}         # 7-digit local
            |
            # Group B: separator-delimited  e.g. +1-415-555-0123 or 415-555-0123
            \+?\d{1,3}[\s\-.]           # country code (or 3-digit area code) + mandatory separator
            \d{2,4}[\s\-.]              # middle group + mandatory separator
            \d{3}[\s\-.]?\d{4}         # 7-digit local
        )
    ",
    )
    .unwrap()
});

static SSN: Lazy<Regex> = Lazy::new(|| Regex::new(r"\b\d{3}-\d{2}-\d{4}\b").unwrap());

// Matches typical 13-19 digit credit card numbers with dashes/spaces every 4.
static CC: Lazy<Regex> = Lazy::new(|| Regex::new(r"\b(?:\d[ \-]?){12,18}\d\b").unwrap());

/// Apply best-effort PII redaction. Order matters: emails first (so phone regex
/// doesn't eat the local-part of an email's digit run), then SSN (specific shape),
/// then CC (long digit runs), then phone.
pub fn redact(input: &str) -> String {
    let s = EMAIL.replace_all(input, "<EMAIL>").into_owned();
    let s = SSN.replace_all(&s, "<SSN>").into_owned();
    let s = CC.replace_all(&s, "<CC>").into_owned();
    PHONE.replace_all(&s, "<PHONE>").into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_emails_phones_ssn_and_credit_cards() {
        let cases = [
            (
                "contact me at sarah@example.com today",
                "contact me at <EMAIL> today",
            ),
            (
                "call (415) 555-0123 or +1-415-555-0123",
                "call <PHONE> or <PHONE>",
            ),
            ("ssn 123-45-6789 then", "ssn <SSN> then"),
            ("card 4111-1111-1111-1111 expires", "card <CC> expires"),
            ("nothing sensitive here", "nothing sensitive here"),
        ];
        for (input, expected) in cases {
            assert_eq!(redact(input), expected, "input: {input}");
        }
    }

    #[test]
    fn idempotent_on_already_redacted_text() {
        let s = "see <EMAIL> and <PHONE>";
        assert_eq!(redact(s), s);
    }

    #[test]
    fn short_numeric_ids_not_redacted() {
        // 8-digit order/invoice IDs must NOT be redacted as phone numbers.
        let cases = ["order #12345678", "invoice 87654321", "ref: 00001234"];
        for input in cases {
            let out = redact(input);
            assert!(
                !out.contains("<PHONE>"),
                "8-digit ID was falsely redacted: input={input:?} out={out:?}"
            );
        }
    }
}
