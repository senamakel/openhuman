impl ApprovalGate {
    /// Resolves the parked oneshot receiver against its wait bound
    /// (`min(park_bound, effective_ttl)`), handling every terminal shape: a
    /// decision arrives, the sender is dropped, the caller-supplied park
    /// bound elapses, or the gate's own TTL elapses. Split out of
    /// [`Self::intercept_audited_inner`] (`gate_intercept.rs`) purely to keep
    /// that file under the repo's per-file line budget — behavior,
    /// including the timeout-vs-decide race handling and event
    /// publication, is unchanged.
    ///
    /// Callers must still run the same post-match teardown
    /// (`waiter_guard.disarm()` + `self.clear_thread(...)`) themselves; this
    /// helper only resolves the `outcome`.
    #[allow(clippy::too_many_arguments)]
    async fn resolve_park_timeout(
        &self,
        request_id: &str,
        tool_name: &str,
        wait: Duration,
        rx: oneshot::Receiver<ApprovalDecision>,
        park_bound_active: bool,
        park_bound_elapsed: &mut bool,
        effective_ttl: Duration,
    ) -> (GateOutcome, Option<String>) {
        match tokio::time::timeout(wait, rx).await {
            Ok(Ok(decision)) => {
                tracing::info!(
                    request_id = %request_id,
                    tool = tool_name,
                    decision = decision.as_str(),
                    "[approval::gate] decision received"
                );
                if decision.is_approve() {
                    (GateOutcome::Allow, Some(request_id.to_string()))
                } else {
                    (
                        GateOutcome::Deny {
                            reason: format!(
                                "{POLICY_DENIED_MARKER} User denied '{tool_name}' execution. Do \
                                 not re-request the same call this turn; take a different approach \
                                 or stop."
                            ),
                        },
                        None,
                    )
                }
            }
            Ok(Err(_canceled)) => {
                // Sender dropped — treat as denial so the agent does
                // not silently no-op.
                tracing::warn!(
                    request_id = %request_id,
                    tool = tool_name,
                    "[approval::gate] decision channel dropped — denying"
                );
                if let Ok(Some(row)) =
                    store::decide(&self.config, request_id, ApprovalDecision::Deny)
                {
                    let route = self.take_request_route(request_id);
                    BUS.publish(DomainEvent::ApprovalDecided {
                        request_id: row.request_id,
                        tool_name: row.tool_name,
                        decision: ApprovalDecision::Deny.as_str().to_string(),
                        thread_id: route.as_ref().and_then(|r| r.thread_id.clone()),
                        client_id: route.as_ref().and_then(|r| r.client_id.clone()),
                        tool_call_id: route.and_then(|r| r.tool_call_id),
                        resolution: Some("cancelled".to_string()),
                        agent_id: row.agent_id.clone(),
                    });
                }
                (
                    GateOutcome::Deny {
                        reason: format!(
                            "{POLICY_DENIED_MARKER} Approval channel for '{tool_name}' closed \
                             before a decision was made."
                        ),
                    },
                    None,
                )
            }
            Err(_elapsed) if park_bound_active => {
                // Caller park bound elapsed (#4756) — NOT the gate's own TTL.
                // Abandon the park cancellation-safely: evict the in-memory
                // waiter and (via `clear_thread` below, on every
                // exit) drop the routing mappings so a later chat/voice reply is
                // not mis-routed to this now-abandoned request. Deliberately do
                // NOT `store::decide(Deny)` — the `pending_approvals` row stays
                // open so a later human card-click still resolves it in the DB
                // and a re-ask sees it already-connected. Signal the elapse so
                // the bounded caller renders its own fast-path result rather than
                // a `Deny`.
                self.evict_waiter(request_id);
                *park_bound_elapsed = true;
                tracing::info!(
                    request_id = %request_id,
                    tool = tool_name,
                    bound_secs = wait.as_secs(),
                    "[approval::gate] caller park bound elapsed — abandoning park (row left \
                     pending for a later card-click; waiter + routing cleared) (#4756)"
                );
                // Placeholder outcome; the bounded caller discards it once
                // `*park_bound_elapsed` is set (returns `None`).
                (
                    GateOutcome::Deny {
                        reason: format!(
                            "{POLICY_DENIED_MARKER} Approval for '{tool_name}' exceeded the caller \
                             park bound ({}s).",
                            wait.as_secs()
                        ),
                    },
                    None,
                )
            }
            Err(_elapsed) => {
                self.evict_waiter(request_id);
                // Race: `decide()` may have committed an Approve in
                // SQLite right as the TTL elapsed. `store::decide(Deny)`
                // has `WHERE decided_at IS NULL` so it won't overwrite,
                // but without a re-read we'd return Deny here while the
                // durable audit row says Approved (CodeRabbit review on
                // #2367). Try to deny; if the row was already decided,
                // honor the persisted decision.
                let denied = store::decide(&self.config, request_id, ApprovalDecision::Deny);
                let persisted = match &denied {
                    Ok(Some(_)) => Some(ApprovalDecision::Deny),
                    Ok(None) => store::get_decision(&self.config, request_id).ok().flatten(),
                    Err(_) => None,
                };
                if matches!(persisted, Some(d) if d.is_approve()) {
                    tracing::info!(
                        request_id = %request_id,
                        tool = tool_name,
                        ttl_secs = effective_ttl.as_secs(),
                        "[approval::gate] timeout race: persisted decision was Approve, honoring approval"
                    );
                    // Fall through (no early return) so `clear_thread` below runs
                    // on this path too — otherwise the stale thread→request
                    // mapping survives and the next yes/no on the thread could be
                    // routed to this already-finished request.
                    (GateOutcome::Allow, Some(request_id.to_string()))
                } else {
                    tracing::warn!(
                        request_id = %request_id,
                        tool = tool_name,
                        ttl_secs = effective_ttl.as_secs(),
                        "[approval::gate] approval timed out, denying"
                    );
                    // Only publish when THIS call is the one that actually
                    // committed the terminal `Deny` (`denied == Ok(Some(_))`).
                    // When `denied` is `Ok(None)` a concurrent `decide()` (or
                    // an `expire_stale` sweep) already resolved and published
                    // this request — publishing again here would double-fire
                    // the socket bridge for a request that already reported
                    // its outcome once, and `take_request_route` would have
                    // nothing left to hand back anyway.
                    if let Ok(Some(row)) = &denied {
                        let route = self.take_request_route(request_id);
                        BUS.publish(DomainEvent::ApprovalDecided {
                            request_id: row.request_id.clone(),
                            tool_name: row.tool_name.clone(),
                            decision: ApprovalDecision::Deny.as_str().to_string(),
                            thread_id: route.as_ref().and_then(|r| r.thread_id.clone()),
                            client_id: route.as_ref().and_then(|r| r.client_id.clone()),
                            tool_call_id: route.and_then(|r| r.tool_call_id),
                            resolution: Some("expired".to_string()),
                            agent_id: row.agent_id.clone(),
                        });
                    }
                    (
                        GateOutcome::Deny {
                            reason: format!(
                                "{POLICY_DENIED_MARKER} Approval for '{tool_name}' timed out after \
                                 {}s: {APPROVAL_UNANSWERED_PHRASE}, so it was not run. Tell the \
                                 user it was not done because the approval was not answered, and \
                                 that they can ask again to retry. Do not re-request the same call \
                                 this turn.",
                                effective_ttl.as_secs()
                            ),
                        },
                        None,
                    )
                }
            }
        }
    }
}
