//! Pregel-style super-step executor.
//!
//! Each super-step runs the current frontier of `(node, state)` pairs — one for
//! a linear chain, several for a fan-out — concurrently, collects their
//! [`Command`]s, and computes the next frontier. Branches that converge on the
//! same node have their states folded via [`GraphState::merge`]. Cycles are
//! allowed (the canonical agent turn loops `dispatch → … → dispatch`); a
//! super-step cap backstops runaways.

use std::collections::BTreeMap;

use super::builder::CompiledGraph;
use super::node::{GraphCancel, NodeCtx, ProgressSink};
use super::state::GraphState;
use crate::openhuman::agent_graph::types::{
    Command, GraphError, GraphRunStatus, InterruptRequest, NodeId, NodeTransition, RunResult, END,
};

/// Render a [`Command`] as a short label for transition records / logs.
fn command_label(cmd: &Command) -> String {
    match cmd {
        Command::Continue => "continue".to_string(),
        Command::Goto(n) => format!("goto:{n}"),
        Command::Fork(ns) => format!("fork:[{}]", ns.join(",")),
        Command::Interrupt(req) => format!("interrupt:{}", req.kind),
        Command::End => "end".to_string(),
    }
}

/// Run `graph` from `frontier` until every branch ends, an interrupt fires, or a
/// guard trips. Shared by both fresh invokes and resumes.
pub(crate) async fn run_from<S: GraphState>(
    graph: &CompiledGraph<S>,
    mut frontier: Vec<(NodeId, S)>,
    run_id: &str,
    sink: &dyn ProgressSink,
    cancel: &GraphCancel,
    start_step: u32,
) -> Result<RunResult<S>, GraphError> {
    let mut transitions: Vec<NodeTransition> = Vec::new();
    let mut step = start_step;
    let mut last_node = frontier
        .first()
        .map(|(n, _)| n.clone())
        .unwrap_or_else(|| graph.entry.clone());
    // Carried so a fully-drained run still returns a meaningful final state.
    let mut final_state: Option<S> = None;

    while !frontier.is_empty() {
        if cancel.is_cancelled() {
            tracing::warn!(run_id, graph = %graph.name, "[agent_graph] run cancelled at step {step}");
            return Err(GraphError::Cancelled);
        }
        if step >= graph.max_steps {
            tracing::error!(
                run_id,
                graph = %graph.name,
                max_steps = graph.max_steps,
                "[agent_graph] super-step cap exceeded — aborting (runaway cycle?)"
            );
            return Err(GraphError::MaxStepsExceeded(graph.max_steps));
        }

        // ── Run every node in the current frontier concurrently. ──
        let mut futs = Vec::with_capacity(frontier.len());
        for (node_id, state) in frontier.drain(..) {
            let node = graph
                .nodes
                .get(&node_id)
                .cloned()
                .ok_or_else(|| GraphError::UnknownNode(node_id.clone()))?;
            let ctx = NodeCtx {
                run_id,
                graph_name: &graph.name,
                step,
                sink,
                cancel,
            };
            futs.push(async move {
                sink.node_entered(run_id, step, &node_id);
                let started = std::time::Instant::now();
                let res = node.run(state, &ctx).await;
                let elapsed_ms = started.elapsed().as_millis() as u64;
                (node_id, res, elapsed_ms)
            });
        }
        let results = futures::future::join_all(futs).await;

        // ── Collect next-frontier requests, merging on convergence. ──
        // BTreeMap keeps deterministic (by node id) ordering for stable
        // transcripts and reproducible tests.
        let mut next: BTreeMap<NodeId, S> = BTreeMap::new();
        let mut push_next =
            |id: NodeId, state: S, merges: &mut Vec<GraphError>| match next.get_mut(&id) {
                Some(existing) => {
                    if let Err(e) = existing.merge(state) {
                        merges.push(GraphError::MergeFailed(e));
                    }
                }
                None => {
                    next.insert(id, state);
                }
            };
        let mut merge_errs: Vec<GraphError> = Vec::new();

        for (node_id, res, elapsed_ms) in results {
            let output = res.map_err(|source| GraphError::NodeFailed {
                node: node_id.clone(),
                source,
            })?;
            last_node = node_id.clone();
            let label = command_label(&output.command);
            sink.node_completed(run_id, step, &node_id, &label, elapsed_ms);
            transitions.push(NodeTransition {
                step,
                node: node_id.clone(),
                command: label.clone(),
                elapsed_ms,
                ts: chrono::Utc::now().to_rfc3339(),
            });
            tracing::debug!(
                run_id,
                graph = %graph.name,
                step,
                node = %node_id,
                command = %label,
                elapsed_ms,
                "[agent_graph] node completed"
            );

            let state = output.state;
            match output.command {
                Command::Interrupt(req) => {
                    // Pause the whole run. resume_to defaults to the static
                    // successor of the interrupting node so resume continues
                    // past it.
                    let resume_to = req
                        .resume_to
                        .clone()
                        .or_else(|| graph.static_next(&node_id, &state).filter(|n| n != END));
                    let req = InterruptRequest { resume_to, ..req };
                    sink.run_paused(run_id, &node_id, &req.kind);
                    tracing::info!(
                        run_id,
                        graph = %graph.name,
                        node = %node_id,
                        kind = %req.kind,
                        "[agent_graph] run paused at human-in-the-loop interrupt"
                    );
                    return Ok(RunResult {
                        state,
                        status: GraphRunStatus::Paused,
                        last_node: node_id,
                        steps: step + 1,
                        interrupt: Some(req),
                        transitions,
                    });
                }
                Command::End => {
                    final_state = Some(merge_into(final_state.take(), state, &mut merge_errs));
                }
                Command::Goto(target) => {
                    if target == END {
                        final_state = Some(merge_into(final_state.take(), state, &mut merge_errs));
                    } else {
                        push_next(target, state, &mut merge_errs);
                    }
                }
                Command::Fork(targets) => {
                    if targets.is_empty() {
                        final_state = Some(merge_into(final_state.take(), state, &mut merge_errs));
                    } else {
                        for t in targets {
                            push_next(t, state.clone(), &mut merge_errs);
                        }
                    }
                }
                Command::Continue => {
                    // Honour an explicit fork edge, else the single static next.
                    if let Some(fork) = graph.fork_targets(&node_id) {
                        for t in fork {
                            push_next(t, state.clone(), &mut merge_errs);
                        }
                    } else {
                        match graph.static_next(&node_id, &state) {
                            Some(n) if n == END => {
                                final_state =
                                    Some(merge_into(final_state.take(), state, &mut merge_errs));
                            }
                            Some(n) => push_next(n, state, &mut merge_errs),
                            None => {
                                // Finish node with no edge — branch ends.
                                final_state =
                                    Some(merge_into(final_state.take(), state, &mut merge_errs));
                            }
                        }
                    }
                }
            }
        }

        if let Some(err) = merge_errs.into_iter().next() {
            return Err(err);
        }

        frontier = next.into_iter().collect();
        step += 1;
    }

    let state = final_state.ok_or_else(|| GraphError::NodeFailed {
        node: last_node.clone(),
        source: anyhow::anyhow!("graph drained without producing a final state"),
    })?;
    tracing::info!(
        run_id,
        graph = %graph.name,
        steps = step,
        last_node = %last_node,
        "[agent_graph] run completed"
    );
    Ok(RunResult {
        state,
        status: GraphRunStatus::Completed,
        last_node,
        steps: step,
        interrupt: None,
        transitions,
    })
}

/// Fold a terminal-branch state into the accumulated final state (last branch to
/// finish merges into the earlier ones).
fn merge_into<S: GraphState>(acc: Option<S>, state: S, errs: &mut Vec<GraphError>) -> S {
    match acc {
        None => state,
        Some(mut existing) => {
            if let Err(e) = existing.merge(state) {
                errs.push(GraphError::MergeFailed(e));
            }
            existing
        }
    }
}
