//! [`ArtifactIndexTocMiddleware`]: render the run's persisted-artifact index
//! into every request as a bounded contents list (issue #6014).

use async_trait::async_trait;

use tinyagents_harness::context::RunContext;
use tinyagents_harness::error::Result as TaResult;
use tinyagents_harness::middleware::Middleware;
use tinyinference::message::Message as TaMessage;
use tinyinference::model::ModelRequest;

use super::message_trim::estimate_text_tokens;

// ── ArtifactIndexTocMiddleware (issue #6014) ─────────────────────────────────

/// Namespace `ToolOutputMiddleware` writes each persisted artifact under.
const ARTIFACT_INDEX_NAMESPACE: &str = "tool_results";

/// The input allowance the reductions divide up when no context window is
/// advertised.
///
/// A window is what every other bound here is derived from, so without one
/// there is nothing to take a share of — and no `ImageAwareMessageTrimMiddleware`
/// installed either, which is precisely why neither share may be left unbounded
/// on that path (CodeRabbit on #6068): these are the two additions nothing
/// downstream can shrink, on the one configuration where nothing downstream is
/// watching. Sized so the contents list's tenth lands on the 512 tokens a
/// realistic handful of artifacts needs.
pub(crate) const NO_WINDOW_ALLOWANCE: u64 = 5_120;

/// The floor under the contents list's proportional share, so a small window
/// still yields a cap rather than collapsing to `0` — which both middlewares
/// read as "unbounded".
const TOC_ALLOWANCE_MIN: u64 = 64;

/// Reserved for the omitted-count line, which cannot be measured before the
/// row cap decides how many rows were dropped. One short sentence.
pub(crate) const FOOTER_ALLOWANCE: u64 = 48;

/// Split a turn's input allowance between the two things that add to the
/// request: the artifact contents list and the wrap-up's result restoration.
///
/// Returns `(toc, restore)`. **Neither is ever `0`** — that is the sentinel both
/// middlewares read as "no cap", so handing it to either is the bug this
/// function exists to make unrepresentable. It bit twice on #6068: first when
/// the two bounds were computed independently and the pair went unbounded, then
/// when the no-window fallback was given to the contents list alone and
/// `saturating_sub` handed restoration the sentinel it had just been rescued
/// from.
///
/// The contents list takes a tenth — generous for the realistic case, firm
/// about the pathological one — floored so a small window cannot round it away,
/// and capped at half so restoration keeps a real share of a small allowance.
pub(crate) fn split_input_allowance(trim_allowance: u64) -> (u64, u64) {
    let total = if trim_allowance == 0 {
        NO_WINDOW_ALLOWANCE
    } else {
        trim_allowance
    };
    let toc = (total / 10)
        .max(TOC_ALLOWANCE_MIN)
        .min(total.div_ceil(2))
        .max(1);
    (toc, total.saturating_sub(toc).max(1))
}

/// Renders the run's persisted-artifact index into the request as a short
/// contents list, so the model can always see what this turn has gathered and
/// where the full copy lives.
///
/// # The reference used to live somewhere it could be destroyed
///
/// A tool result over the per-result budget is written to disk and replaced by
/// a preview plus an `artifact_path` pointer (`apply_per_result_persistence`).
/// That pointer's only home was the tool-result message itself — and tool-result
/// messages are exactly what the two reduction steps act on. Microcompact
/// replaces a body with `CLEARED_PLACEHOLDER`; compression folds the older slice
/// into a summary that *should* carry paths forward ("prefer concrete facts —
/// paths, names, values") but is a model call, not a guarantee.
///
/// Either way the outcome is the same and it is silent: the data sits intact on
/// disk with nothing in context saying it exists. Every component did its job —
/// the result was persisted, the transcript was reduced — and the turn quietly
/// lost the ability to reach its own findings. No log fires, because nothing
/// failed.
///
/// The index has been maintained all along (`ToolResultArtifactIndexStore`,
/// registered on `RunContext.stores`, written on every persist). Nothing read
/// it. This reads it.
///
/// # Why re-rendered rather than injected once
///
/// The contents list is built fresh into each request and never enters the
/// loop's own transcript, which is what makes it un-destroyable rather than
/// merely durable: there is no stored copy for a later pass to blank, fold or
/// evict, and it cannot accumulate into a stack of stale lists. It is also
/// always current — an artifact persisted on the previous round appears on the
/// next call with no bookkeeping.
///
/// Emitted as a **system** message for the same reason: compression keeps every
/// system message verbatim and the trim never drops one, so the one thing that
/// says where the data went is the one thing the ladder may not take. It is
/// appended at the tail rather than the head so the cacheable prompt prefix is
/// untouched.
pub(crate) struct ArtifactIndexTocMiddleware {
    /// This middleware's share of the turn's input allowance (a tenth, split at
    /// the install site so restoration and this list cannot each claim the
    /// whole). `0` disables the cap — no advertised window, and no trim
    /// installed either.
    input_budget: u64,
}

impl ArtifactIndexTocMiddleware {
    pub(crate) fn new(input_budget: u64) -> Self {
        Self { input_budget }
    }
}

#[async_trait]
impl Middleware<()> for ArtifactIndexTocMiddleware {
    fn name(&self) -> &str {
        "artifact_index_toc"
    }

    async fn before_model(
        &self,
        ctx: &mut RunContext<()>,
        _state: &(),
        request: &mut ModelRequest,
    ) -> TaResult<()> {
        let Some(store) = ctx.stores.get(
            crate::agent::harness::tool_result_artifacts::TINYAGENTS_TOOL_RESULT_ARTIFACT_STORE,
        ) else {
            return Ok(());
        };
        // A read failure and an empty index produce the same contents list —
        // none — but they are not the same event: the second is the ordinary
        // case, the first means every persisted result is unreachable for this
        // call with nothing said about it. Degrading is still right (a contents
        // list is not worth failing a turn over), so the difference has to show
        // up in the log or it shows up nowhere (CodeRabbit on #6068).
        let keys = match store.list(ARTIFACT_INDEX_NAMESPACE).await {
            Ok(keys) => keys,
            Err(err) => {
                tracing::warn!(
                    error = %err,
                    "[tinyagents::mw] could not read the persisted-artifact index; this call gets \
                     no contents list and cannot see what was offloaded"
                );
                return Ok(());
            }
        };
        if keys.is_empty() {
            // No result has been offloaded, so there is nothing to point at and
            // no reason to spend context saying so.
            return Ok(());
        }

        let mut rows: Vec<String> = Vec::new();
        for key in &keys {
            let Ok(Some(entry)) = store.get(ARTIFACT_INDEX_NAMESPACE, key).await else {
                continue;
            };
            let tool = entry.get("tool").and_then(|v| v.as_str()).unwrap_or("tool");
            let Some(path) = entry.get("artifact_path").and_then(|v| v.as_str()) else {
                continue;
            };
            let bytes = entry
                .get("original_bytes")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0);
            rows.push(format!("- `{tool}` → `{path}` ({bytes} bytes)"));
        }
        if rows.is_empty() {
            return Ok(());
        }
        // Sorted so the list is stable between calls when nothing was added —
        // the index is a `HashMap`, whose iteration order is not. An unstable
        // ordering would rewrite this message on every call for no reason,
        // costing cache and making the diff unreadable in a trace.
        rows.sort();
        let total = rows.len();
        // Cap the list against the input allowance (CodeRabbit on #6068).
        //
        // This message is a system message precisely so the ladder cannot take
        // it — compression keeps system messages verbatim and the trim never
        // evicts one — which means nothing downstream can shrink it either. A
        // turn that persisted enough oversized results would otherwise grow an
        // un-evictable message without limit, and the guarantee that made the
        // pointers safe would be the thing that broke the request.
        //
        // A tenth of the input allowance: generous for the realistic case (a
        // handful of artifacts is a few hundred bytes) and firm about the
        // pathological one. The omitted count is reported rather than the list
        // silently ending, so the model knows more exist — the same disclosure
        // rule the rest of this ladder follows, and the reason the artifacts are
        // findable at all.
        let header = format!(
            "## Stored results from this turn\n\n\
             {total} tool result(s) were too large to keep inline and were written to disk. The \
             text you saw for them is a preview; the full content is at the path below and can be \
             read with the file-reading tool when you need detail the preview does not carry.\n\n"
        );
        // One line that still names the count, for a share too small to hold
        // the header at all (CodeRabbit on #6068). Truncating to zero rows was
        // half the fix: the fixed text is ~118 tokens and the floor a small
        // window gets is 64, so the message still cleared its share with no
        // rows in it.
        let compact = format!(
            "_{total} tool result(s) were written to disk — ask by tool name to locate one._"
        );
        if self.input_budget > 0 {
            let cap = self.input_budget.max(1);
            let fixed = estimate_text_tokens(&header).saturating_add(FOOTER_ALLOWANCE);
            if fixed > cap {
                // Even the compact line has to fit. Saying nothing loses the
                // pointer, which is bad; pushing an unshrinkable system message
                // over the bound makes the trim evict transcript instead, which
                // is worse.
                if estimate_text_tokens(&compact) <= cap {
                    request.messages.push(TaMessage::system(compact));
                }
                return Ok(());
            }
            // Already this middleware's share of the turn's allowance — the
            // split happens once, at the install site, so the two things that
            // add to the request cannot each spend the whole of it.
            // Seeded with the fixed text, so the cap bounds the whole message
            // rather than the rows alone (CodeRabbit on #6068). The header is a
            // paragraph; against the 64-token floor a small window gets, it is
            // comparable to the rows it introduces. `FOOTER_ALLOWANCE` stands in
            // for the omitted-count line, which cannot be measured before the
            // count is known — it is one short sentence, and reserving it when
            // nothing is omitted only spends a row.
            let mut used: u64 = estimate_text_tokens(&header).saturating_add(FOOTER_ALLOWANCE);
            let mut kept = 0usize;
            for row in &rows {
                used = used.saturating_add(estimate_text_tokens(row));
                if used > cap {
                    break;
                }
                kept += 1;
            }
            // No `max(1)`: once `used` is seeded with the fixed text, a share
            // smaller than that text leaves `kept == 0`, and forcing a row then
            // puts the whole message over a bound nothing downstream can shrink
            // — this is a system message, so compression keeps it and the trim
            // never evicts it, and the overshoot is charged against the
            // transcript instead (CodeRabbit on #6068). The header and the
            // omitted count still say the results exist and how to ask for one.
            rows.truncate(kept);
        }
        let shown = rows.len();
        let omitted = total - shown;
        let count = total;
        tracing::debug!(
            artifacts = count,
            "[tinyagents::mw] rendering the persisted-artifact contents list"
        );
        request.messages.push(TaMessage::system(format!(
            "{header}{}{}",
            rows.join("\n"),
            if omitted > 0 {
                format!(
                    "\n\n_({omitted} more stored result(s) not listed here — ask for the one you \
                     need by tool name and it can be located.)_"
                )
            } else {
                String::new()
            }
        )));
        Ok(())
    }
}
