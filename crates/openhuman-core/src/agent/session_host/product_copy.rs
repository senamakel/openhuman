//! OpenHuman's user-facing copy for generic TinyAgents session operations.
//!
//! Rendering, classification and validation live in TinyAgents. This module
//! supplies the product wording at those seams.

/// OpenHuman copy for bounded summaries of a turn stopped by a breaker.
pub(crate) const OPENHUMAN_STOP_COPY: tinyagents_harness::turn_stop::StopCopy =
    tinyagents_harness::turn_stop::StopCopy {
        opening_template: "I stopped this turn early because {reason}.\n",
        what_happened_heading: "\n**What happened**\n",
        success_template: "- `{name}` succeeded{count}",
        failure_template: "- `{name}` failed{count}{error}",
        hidden_calls_template: "- …and {count} more tool call(s)",
        proceed_prompt: "\nTell me how you'd like to proceed, or ask me to try again.",
    };

/// OpenHuman's prompt and fallback wording for generic TinyAgents close paths.
pub(crate) const OPENHUMAN_CLOSE_COPY: tinyagents_harness::close::CloseCopy =
    tinyagents_harness::close::ENGLISH_CLOSE_COPY;

/// The phrases that mark `copy`'s directions as echoed back to the user.
///
/// The harness derives them from `copy.instruction_needles`, a static list an
/// embedder's copy usually inherits from the English default. The instructions
/// the turn is actually given are added here, so a copy that rewrites one is
/// still recognised when the model repeats it.
pub(crate) fn close_needles(
    copy: &tinyagents_harness::close::CloseCopy,
    stop_reason: Option<&str>,
) -> Vec<String> {
    let mut sources: Vec<&str> = copy.instruction_needles.to_vec();
    for instruction in [
        copy.max_iteration_instruction,
        copy.final_write_instruction,
        copy.final_answer_instruction,
        copy.truncated_answer_instruction,
    ] {
        if !sources.contains(&instruction) {
            sources.push(instruction);
        }
    }
    if let Some(reason) = stop_reason {
        sources.push(reason);
    }
    tinyagents_harness::close::harness_instruction_needles(&sources, copy.minimum_quoted_words)
}

/// Whether `output` is `copy`'s deterministic "finished without a result"
/// summary, judged by the wording its template opens with.
pub(crate) fn is_final_summary(copy: &tinyagents_harness::close::CloseCopy, output: &str) -> bool {
    let opening = copy
        .final_summary_template
        .split('{')
        .next()
        .unwrap_or_default();
    !opening.trim().is_empty() && output.starts_with(opening)
}

/// Capture bounded result text for the OpenHuman checkpoint and close paths.
pub(crate) fn close_results(
    outcomes: &[tinyagents_harness::turn_outcome::ToolCallOutcome],
) -> Vec<tinyagents_harness::close::CloseToolResult> {
    tinyagents_harness::close::results_from_tool_outcomes_with_options(
        outcomes,
        800,
        "… [truncated]",
    )
}

/// Render captured results with OpenHuman's established block format.
pub(crate) fn render_close_results(
    results: &[tinyagents_harness::close::CloseToolResult],
    total_budget: usize,
) -> String {
    tinyagents_harness::close::render_tool_results_with_options(
        results,
        tinyagents_harness::close::CloseResultRenderOptions {
            per_result_cap: 800,
            truncation_marker: "… [truncated]",
            total_budget,
            render_block: |result: &tinyagents_harness::close::CloseToolResult| {
                let status = if result.success { "ok" } else { "failed" };
                let mut block = format!("\n- `{}` — {}\n", result.name, status);
                for line in result.content.lines() {
                    block.push_str("  > ");
                    block.push_str(line);
                    block.push('\n');
                }
                block
            },
            omitted_notice: |count| {
                if count == 0 {
                    String::new()
                } else {
                    format!(
                        "\n_({count} earlier tool result(s) omitted for length — the most recent are shown.)_\n"
                    )
                }
            },
        },
    )
}

#[cfg(test)]
#[path = "product_copy_tests.rs"]
mod tests;
