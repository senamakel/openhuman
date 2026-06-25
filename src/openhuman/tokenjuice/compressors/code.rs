//! Source-code compressor — keep signatures, collapse bodies.
//!
//! Inspired by Headroom's `CodeCompressor`. The goal is to keep the structural
//! skeleton an agent needs to navigate a file — imports, type/function/class
//! signatures, top-level constants — while collapsing the deep bodies that
//! dominate byte count.
//!
//! This module currently ships the language-agnostic **brace-depth heuristic**:
//! lines at brace nesting depth 0–1 are kept; deeper bodies collapse to a
//! `{ … N lines … }` placeholder. Lines carrying error/TODO markers are always
//! kept. A higher-fidelity tree-sitter path (Rust/TS/Python) is layered on in a
//! follow-up slice and selected by language; until then every language uses the
//! heuristic. The router offloads the original to CCR for exact recovery.

use async_trait::async_trait;
use std::fmt::Write as _;

use super::Compressor;
use crate::openhuman::tokenjuice::types::{
    CompressInput, CompressOptions, CompressOutput, CompressorKind,
};

/// Bodies with more than this many collapsed lines get a placeholder; shorter
/// ones are kept verbatim (collapsing tiny bodies isn't worth the marker).
pub const MIN_BODY_LINES_TO_COLLAPSE: usize = 4;

/// Markers that force a line to be kept even inside a deep body.
const KEEP_MARKERS: &[&str] = &["TODO", "FIXME", "XXX", "error", "panic", "unsafe"];

pub struct CodeCompressor;

#[async_trait]
impl Compressor for CodeCompressor {
    fn kind(&self) -> CompressorKind {
        CompressorKind::Code
    }

    async fn compress(
        &self,
        input: &CompressInput<'_>,
        _opts: &CompressOptions,
    ) -> Option<CompressOutput> {
        compress_heuristic(input.content)
    }
}

/// Language-agnostic brace-depth compressor. Keeps lines at depth ≤ 1, collapses
/// deeper runs. Returns `None` if it wouldn't shrink the content.
pub fn compress_heuristic(content: &str) -> Option<CompressOutput> {
    let lines: Vec<&str> = content.lines().collect();
    if lines.len() < 12 {
        return None;
    }

    let mut out = String::with_capacity(content.len() / 2 + 64);
    let mut depth: i32 = 0;
    let mut collapsed: Vec<&str> = Vec::new();

    let flush = |out: &mut String, collapsed: &mut Vec<&str>| {
        if collapsed.is_empty() {
            return;
        }
        if collapsed.len() >= MIN_BODY_LINES_TO_COLLAPSE {
            let _ = writeln!(out, "    {{ … {} line(s) … }}", collapsed.len());
        } else {
            for l in collapsed.iter() {
                let _ = writeln!(out, "{l}");
            }
        }
        collapsed.clear();
    };

    for line in &lines {
        let (opens, closes) = brace_delta(line);
        let start_depth = depth;
        // A line that carries signal is always kept, regardless of depth.
        let force_keep = KEEP_MARKERS.iter().any(|m| line.contains(m));

        // Keep top-level lines (depth 0) — imports, signatures, the line that
        // opens a block — and collapse the block body (depth ≥ 1). Short bodies
        // (e.g. small struct field lists) stay verbatim via the flush threshold,
        // so struct/enum fields survive while long function bodies collapse.
        if start_depth == 0 || force_keep {
            flush(&mut out, &mut collapsed);
            let _ = writeln!(out, "{line}");
        } else {
            collapsed.push(line);
        }
        depth += opens - closes;
        if depth < 0 {
            depth = 0;
        }
    }
    flush(&mut out, &mut collapsed);

    let out = out.trim_end().to_string();
    if out.len() >= content.len() {
        return None;
    }
    log::debug!(
        "[tokenjuice][code] heuristic {} -> {} bytes ({} lines)",
        content.len(),
        out.len(),
        lines.len()
    );
    Some(CompressOutput::lossy(out, CompressorKind::Code))
}

/// Count `{`/`}` (and `(`/`)`) on a line, ignoring those inside string/char
/// literals and line comments — a cheap approximation good enough for the
/// depth heuristic.
fn brace_delta(line: &str) -> (i32, i32) {
    let mut opens = 0i32;
    let mut closes = 0i32;
    let mut in_str: Option<char> = None;
    let mut prev = '\0';
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match in_str {
            Some(q) => {
                if c == q && prev != '\\' {
                    in_str = None;
                }
            }
            None => match c {
                '"' | '\'' | '`' => in_str = Some(c),
                '/' if chars.peek() == Some(&'/') => break, // line comment
                '#' => break,                               // python/shell comment
                '{' => opens += 1,
                '}' => closes += 1,
                _ => {}
            },
        }
        prev = c;
    }
    (opens, closes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_signatures_collapses_bodies() {
        let mut src = String::from("use std::collections::HashMap;\n\n");
        src.push_str("pub fn process(items: &[i32]) -> i32 {\n");
        for i in 0..30 {
            src.push_str(&format!("        let tmp_{i} = items.iter().sum::<i32>() + {i};\n"));
        }
        src.push_str("        tmp_0\n}\n\n");
        src.push_str("struct Config {\n    name: String,\n    size: usize,\n}\n");
        let out = compress_heuristic(&src).expect("compresses");
        assert!(out.lossy);
        assert!(out.text.contains("pub fn process"), "signature kept:\n{}", out.text);
        assert!(out.text.contains("struct Config"));
        assert!(out.text.contains("line(s) …"), "body collapsed:\n{}", out.text);
        assert!(!out.text.contains("tmp_15"), "deep body should be collapsed");
        assert!(out.text.len() < src.len());
    }

    #[test]
    fn short_file_passes_through() {
        let src = "fn a() {}\nfn b() {}\n";
        assert!(compress_heuristic(src).is_none());
    }

    #[test]
    fn keeps_marker_lines_in_body() {
        let mut src = String::from("fn f() {\n");
        for i in 0..20 {
            if i == 10 {
                src.push_str("        // TODO: handle the edge case here\n");
            } else {
                src.push_str(&format!("        do_thing({i});\n"));
            }
        }
        src.push_str("}\n");
        let out = compress_heuristic(&src).expect("compresses");
        assert!(out.text.contains("TODO"), "marker line kept:\n{}", out.text);
    }
}
