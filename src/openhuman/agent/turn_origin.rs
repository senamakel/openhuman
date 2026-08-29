//! Agent turn origin — the trust/routing label attached to every agent
//! `run_turn` invocation. Read by [`crate::openhuman::security::approval::ApprovalGate`]
//! and [`crate::openhuman::tools::agent_policy::ToolPolicyEngine`] to make
//! consistent decisions across web, channel, subconscious, and cron entry
//! points without relying on the *absence* of other task-locals as a signal.
//!
//! Every entry point that drives the agent loop ([`crate::openhuman::web_chat`],
//! [`crate::openhuman::channels::runtime::dispatch`],
//! [`crate::openhuman::cron`], CLI) MUST scope a real [`AgentTurnOrigin`]
//! around its `run_turn` invocation. Any path that fails to do so is treated
//! as [`AgentTurnOrigin::Unknown`] by the gate and the call fails closed.

/// Identifies who scheduled the current agent turn so the approval gate can
/// pick the correct policy: surface to the user, persist for an
/// out-of-band approval surface, run trusted-automation through, or fail
/// closed.
///
/// This is a typed task-local label, not a credential — it is set by the
/// entry point that owns the turn and read by [`crate::openhuman::security::approval`]
/// alongside the existing per-turn chat context.
#[derive(Clone, Debug)]
pub enum AgentTurnOrigin {
    /// Live user chat in the desktop / web UI. The existing
    /// [`crate::openhuman::security::approval::ApprovalChatContext`] task-local is
    /// scoped alongside this so the approval gate has a thread / client to
    /// route the prompt back to.
    WebChat {
        thread_id: String,
        client_id: String,
        /// Per-turn request id, when the caller has one. Used by internal
        /// observers to correlate a live progress bridge with the durable
        /// tinyagents journal stream for the same turn.
        request_id: Option<String>,
    },
    /// Inbound message from a non-web channel (Telegram / Discord / Slack /
    /// Yuanbao / etc.). External-effect tools must persist a
    /// `pending_approvals` row for the audit trail; the parked future will
    /// TTL-deny because no caller picks up the chat-routed approval on this
    /// surface yet — which is the correct fail-closed default for remote
    /// inputs.
    ///
    /// `sender` carries the per-user identity (Discord user id, Telegram
    /// from_account, Slack user id, etc.) when available so per-user
    /// isolation invariants survive into the gate's audit trail. Legacy
    /// publishers that don't surface the sender pass `None`; the gate still
    /// fails closed because the channel input is remote-untrusted regardless
    /// of which sender produced it. Distinct senders in the same shared
    /// channel produce distinct origins so a co-channel attacker cannot
    /// resume a victim's parked approval flow.
    ExternalChannel {
        channel: String,
        sender: Option<String>,
        reply_target: String,
        message_id: String,
    },
    /// Internal automation the user explicitly authorized (cron job the
    /// user created, subconscious tick on internal-only memory). `source`
    /// carries enough info for the gate to apply the right per-source
    /// allowlist.
    TrustedAutomation {
        job_id: String,
        source: TrustedAutomationSource,
    },
    /// Command-line / sub-agent / one-off internal invocation.
    Cli,
    /// Unlabelled — gate fails closed. Every entry point MUST scope a real
    /// origin before invoking the agent.
    Unknown,
}

/// Sub-classification for [`AgentTurnOrigin::TrustedAutomation`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TrustedAutomationSource {
    /// Cron job created and authorized by the user.
    Cron,
    /// Subconscious tick whose memory context is internal-only.
    Subconscious,
    /// Subconscious tick whose memory context includes chunks ingested
    /// from an external sync source (Gmail / Slack / Notion / etc.).
    /// Treated as untrusted: external-effect tool surface blocked.
    SubconsciousTainted,
    /// Autonomous continuation of a thread goal: the heartbeat injected a turn
    /// to keep working an idle `active` goal the user explicitly created.
    GoalContinuation,
    /// A saved, enabled `flows::Flow` (tinyflows workflow) executing via
    /// `flows::ops::flows_run` / `flows_resume` (issue B2, see
    /// `my_docs/ohxtf/b2-triggers-trust/01-triggers-and-trust.md` §3). The
    /// flow's `tool_call`/`http_request` nodes were pre-declared (their
    /// `slug`/`url` are static graph config, never `=`-expression evaluated
    /// in tinyflows 0.2 — see `my_docs/ohxtf/commons/12-node-catalog-0.2.md`)
    /// and validated when the flow was saved, so the *action* carries a trust
    /// root the same way a user-authored cron job's prompt does. The runtime
    /// trigger payload (webhook body, Composio event, …) stays untrusted —
    /// nothing in it can introduce a *new* action, only feed the pre-declared
    /// one's arguments.
    Workflow {
        /// Mirrors `Flow::require_approval`: when `true` the gate does NOT
        /// auto-allow this trust root — every external_effect call still
        /// parks for a real decision (same shape as `GoalContinuation`),
        /// letting a user force human review on a specific flow's outbound
        /// actions regardless of the trust root above.
        require_approval: bool,
    },
}

impl AgentTurnOrigin {
    /// A PII-free classification label safe for `info`-level logs and audit
    /// trails — the variant name (and, for `TrustedAutomation`, its `source`
    /// sub-kind), never an identifying field. Use this instead of `{:?}` /
    /// `?origin` anywhere the log line isn't gated to `debug`/`trace`:
    /// `WebChat.thread_id`/`client_id`, `ExternalChannel.sender`/
    /// `reply_target`/`message_id`, and `TrustedAutomation.job_id` can carry
    /// user- or channel-identifying data that must not land at `info`.
    pub fn class(&self) -> String {
        match self {
            AgentTurnOrigin::WebChat { .. } => "WebChat".to_string(),
            AgentTurnOrigin::ExternalChannel { channel, .. } => {
                format!("ExternalChannel({channel})")
            }
            AgentTurnOrigin::TrustedAutomation { source, .. } => {
                format!("TrustedAutomation({source:?})")
            }
            AgentTurnOrigin::Cli => "Cli".to_string(),
            AgentTurnOrigin::Unknown => "Unknown".to_string(),
        }
    }
}

tokio::task_local! {
    /// Per-turn agent origin. Scoped by entry points (web channel, channel
    /// runtime dispatch, subconscious loop, cron scheduler, CLI) around the
    /// `run_turn` invocation. Read by the approval gate to make
    /// origin-aware decisions.
    pub static AGENT_TURN_ORIGIN: AgentTurnOrigin;
}

/// Scope `origin` for the duration of `fut`. Mirrors the existing
/// [`crate::openhuman::security::approval::APPROVAL_CHAT_CONTEXT`] scope pattern.
///
/// The inner future is `Box::pin`-ed before being handed to the task-local
/// scope so the combined `with_origin(... scope(... run_turn(...)))` future
/// state machine stays heap-allocated. The agent loop downstream of this
/// scope can be deep (tool dispatch, recursive sub-agent invocations, LLM
/// streaming), and stacking two task-local scopes plus the agent loop on a
/// 2 MiB worker stack reliably blows the test runtime — same shape as the
/// fix in PR #3151. Box-pinning here is the single-point remediation that
/// covers every caller (web channel, channel runtime, subconscious, cron,
/// CLI).
pub async fn with_origin<F: std::future::Future>(origin: AgentTurnOrigin, fut: F) -> F::Output {
    AGENT_TURN_ORIGIN.scope(origin, Box::pin(fut)).await
}

/// Try to read the current origin. Returns `None` when no caller scoped one
/// (legacy callers that haven't been migrated yet — the gate maps this to
/// [`AgentTurnOrigin::Unknown`] / fail-closed).
pub fn current() -> Option<AgentTurnOrigin> {
    AGENT_TURN_ORIGIN.try_with(|o| o.clone()).ok()
}

/// Capture the ambient origin so it can be carried across a `tokio::spawn`
/// boundary by [`with_inherited_origin`].
///
/// This is exactly [`current()`] — it exists as a named pair with
/// `with_inherited_origin` so the capture/re-scope idiom is greppable at every
/// delegation site, and so the capture is obviously required to happen on the
/// *parent* task (task-locals do not cross `tokio::spawn`; calling this inside
/// the spawned future always yields `None`).
pub fn capture() -> Option<AgentTurnOrigin> {
    current()
}

/// Re-scope a [`capture()`]d origin around `fut` on a freshly-spawned task.
///
/// # Why this is inherit-only
///
/// `AGENT_TURN_ORIGIN` is a `tokio` task-local, so it is **lost** the moment
/// work moves onto a new task via `tokio::spawn`. An async sub-agent, team
/// member, or workflow phase therefore runs unlabelled, the approval gate reads
/// [`AgentTurnOrigin::Unknown`], and every `external_effect` tool (shell/exec)
/// is refused. Re-establishing the parent's label is the fix.
///
/// It re-establishes the parent's label and **nothing else**:
///
/// * `Some(origin)` — scope that exact origin, unchanged. A worker descending
///   from an [`AgentTurnOrigin::ExternalChannel`] turn stays `ExternalChannel`
///   (remote, untrusted); it is never promoted to `Cli` or any other origin
///   just because it now runs on a background task. Delegation must not be a
///   privilege-escalation primitive.
/// * `None` — run `fut` with **no** scope at all. The spawned task stays
///   unlabelled and the gate keeps failing closed exactly as it does today.
///   Never substitute a default origin here: fabricating a label for an
///   unlabelled parent would hand every unlabelled call site in the process a
///   trust root it never earned.
///
/// Capture on the parent task *before* the `tokio::spawn`, move the
/// `Option<AgentTurnOrigin>` into the spawned future, and wrap the future's
/// body:
///
/// ```ignore
/// let inherited = turn_origin::capture();
/// tokio::spawn(async move {
///     turn_origin::with_inherited_origin(inherited, async move { /* agent work */ }).await
/// });
/// ```
pub async fn with_inherited_origin<F: std::future::Future>(
    captured: Option<AgentTurnOrigin>,
    fut: F,
) -> F::Output {
    match captured {
        // Box-pinned by `with_origin` for the same stack-depth reason
        // documented there — the agent loop downstream can be very deep.
        Some(origin) => with_origin(origin, fut).await,
        // Deliberately unlabelled: fail-closed is the correct default.
        None => fut.await,
    }
}

/// Carry the origin scoped **right now** into a future that will run on
/// another task.
///
/// A `tokio::task_local` does not cross `tokio::spawn`: a detached sub-agent
/// (`spawn_async_subagent`, the orchestration `spawn_agent` task) starts on a
/// fresh task where [`current`] is `None`, so every external-effect tool it
/// calls reaches the approval gate as [`AgentTurnOrigin::Unknown`] and is
/// refused — even though the parent turn that delegated the work was properly
/// labelled. That is the same failure mode
/// [`fork_context::with_parent_context`](crate::openhuman::agent::harness::fork_context)
/// and [`thread_context::with_thread_id`](crate::openhuman::agent::tinyagents::thread_context)
/// already re-install explicitly at those spawn sites; the origin is the third
/// thing that has to travel with them.
///
/// **Call this on the parent task**, i.e. build the future *before* handing it
/// to `tokio::spawn` — the origin is read when this function is called, not
/// when the returned future is first polled:
///
/// ```ignore
/// tokio::spawn(turn_origin::propagate(async move { run_subagent(..).await }));
/// ```
///
/// Fail-closed is preserved: with no ambient origin nothing is scoped, so the
/// child still lands on `Unknown` rather than inheriting a label nobody set.
/// This only ever *carries* a decision the parent entry point already made — it
/// cannot manufacture trust that did not exist on the spawning task.
pub fn propagate<F: std::future::Future>(fut: F) -> impl std::future::Future<Output = F::Output> {
    let captured = current();
    async move {
        match captured {
            Some(origin) => with_origin(origin, fut).await,
            None => fut.await,
        }
    }
}

/// `tokio::spawn`, with the current turn origin carried onto the new task.
///
/// # Why this exists when [`propagate`] already does the carrying
///
/// [`propagate`] and [`capture`] read the origin **when they are called**, which
/// has to be on the spawning task — a task-local is already gone by the time the
/// spawned future is first polled. Both of these compile, neither warns, and
/// only the first is right:
///
/// ```ignore
/// tokio::spawn(turn_origin::propagate(work));              // correct
/// tokio::spawn(async move { turn_origin::propagate(work).await });  // silently Unknown
/// ```
///
/// The second captures inside the new task, where [`current`] is already `None`,
/// so it scopes nothing and every external-effect tool the child calls is
/// refused by the approval gate. The existing call sites get this right only
/// because each one carries a hand-written comment saying to capture *here, on
/// the spawning task* — correctness resting on reviewer attention at every
/// future site.
///
/// This helper removes the ordering from the caller's hands: the capture happens
/// inside, before the spawn, and there is no argument order that can get it
/// wrong.
///
/// # Fail-closed is preserved
///
/// With no ambient origin nothing is scoped, so the child lands on
/// [`AgentTurnOrigin::Unknown`] exactly as a bare `tokio::spawn` would. This
/// only ever *carries* a decision some entry point already made; it cannot
/// manufacture a trust root. See [`propagate`], which does the actual work.
///
/// # What it does not carry
///
/// Only the origin. A delegated agent turn usually also needs
/// [`turn_workspace::propagate`](super::turn_workspace) and the harness fork
/// context; those are separate wrappers and still have to be applied around the
/// future passed in here.
///
/// ```ignore
/// let join = turn_origin::spawn(turn_workspace::propagate(async move { .. }));
/// ```
pub fn spawn<F>(fut: F) -> tokio::task::JoinHandle<F::Output>
where
    F: std::future::Future + Send + 'static,
    F::Output: Send + 'static,
{
    // `propagate` is evaluated here, on the caller's task, which is the whole
    // point of routing through this function.
    tokio::spawn(propagate(fut))
}

/// `tokio::spawn` for work that must deliberately **not** carry the caller's
/// origin, naming why.
///
/// Dropping the origin is sometimes right — a detached background job that is
/// not a continuation of the caller's turn should not inherit that turn's
/// authority. The problem is that a bare `tokio::spawn` looks identical whether
/// the author decided that or simply did not think about it, so a reviewer
/// cannot tell a deliberate choice from a regression.
///
/// This is a plain `tokio::spawn` — the behaviour is the same — but the name and
/// the `reason` make the choice explicit at the call site and greppable across
/// the tree. The reason is emitted at `trace` so a live process can be asked
/// which spawns dropped their label.
///
/// Prefer [`spawn`] unless the work genuinely is not a continuation of the
/// caller's turn.
pub fn spawn_unlabelled<F>(reason: &'static str, fut: F) -> tokio::task::JoinHandle<F::Output>
where
    F: std::future::Future + Send + 'static,
    F::Output: Send + 'static,
{
    tracing::trace!(
        reason,
        parent_origin = ?current().as_ref().map(AgentTurnOrigin::class),
        "[turn_origin] spawning without the caller's origin"
    );
    tokio::spawn(fut)
}

/// Read the ambient web-chat `request_id` for the current turn, when one was
/// scoped by an [`AgentTurnOrigin::WebChat`] entry point. `None` for every
/// other origin (channel / cron / CLI / sub-agent) and outside any scope —
/// those turns are not request-scoped, so their transcript lines carry no
/// turn-boundary marker.
pub fn current_request_id() -> Option<String> {
    match current() {
        Some(AgentTurnOrigin::WebChat { request_id, .. }) => request_id,
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn with_origin_scopes_correctly_and_unscopes_on_exit() {
        // Outside any scope: current() returns None.
        assert!(current().is_none());

        let observed = with_origin(AgentTurnOrigin::Cli, async {
            // Inside the scope: current() returns the scoped origin.
            current()
        })
        .await;
        assert!(matches!(observed, Some(AgentTurnOrigin::Cli)));

        // After the scope exits, current() is None again.
        assert!(current().is_none());
    }

    /// The defect this helper fixes: a `tokio::spawn`ed delegation loses the
    /// task-local, so the gate saw `Unknown` and refused every shell/exec tool.
    /// Capturing on the parent and re-scoping inside the spawned task restores
    /// the label.
    #[tokio::test]
    async fn inherited_origin_crosses_a_spawn_boundary() {
        let observed = with_origin(AgentTurnOrigin::Cli, async {
            // Capture happens on the still-scoped parent task.
            let captured = capture();
            tokio::spawn(async move {
                // Without the re-scope this is `None` (task-locals don't cross
                // `tokio::spawn`).
                with_inherited_origin(captured, async { current() }).await
            })
            .await
            .expect("spawned task panicked")
        })
        .await;
        assert!(
            matches!(observed, Some(AgentTurnOrigin::Cli)),
            "expected the parent's Cli origin to be inherited, got {observed:?}"
        );
    }

    /// Fail-closed is preserved: an unlabelled parent produces an unlabelled
    /// child. The helper must never fabricate an origin.
    #[tokio::test]
    async fn inherited_origin_stays_unlabelled_without_an_outer_scope() {
        let captured = capture();
        assert!(captured.is_none(), "test precondition: no ambient scope");

        let observed =
            tokio::spawn(async move { with_inherited_origin(captured, async { current() }).await })
                .await
                .expect("spawned task panicked");

        assert!(
            observed.is_none(),
            "unlabelled parent must stay unlabelled, got {observed:?}"
        );
    }

    /// A remote-untrusted origin is inherited *as itself* — delegation is not a
    /// privilege-escalation primitive, so no upgrade to `Cli` may happen.
    #[tokio::test]
    async fn inherited_origin_preserves_a_non_cli_origin_verbatim() {
        let observed = with_origin(
            AgentTurnOrigin::ExternalChannel {
                channel: "telegram".into(),
                sender: Some("u-42".into()),
                reply_target: "chat-7".into(),
                message_id: "m-9".into(),
            },
            async {
                let captured = capture();
                tokio::spawn(
                    async move { with_inherited_origin(captured, async { current() }).await },
                )
                .await
                .expect("spawned task panicked")
            },
        )
        .await;

        match observed {
            Some(AgentTurnOrigin::ExternalChannel {
                channel,
                sender,
                reply_target,
                message_id,
            }) => {
                assert_eq!(channel, "telegram");
                assert_eq!(sender.as_deref(), Some("u-42"));
                assert_eq!(reply_target, "chat-7");
                assert_eq!(message_id, "m-9");
            }
            other => panic!("expected ExternalChannel inherited verbatim, got {other:?}"),
        }
    }

    /// Regression: a detached sub-agent (`spawn_async_subagent`, the
    /// orchestration spawn task) starts on a fresh task, and without explicit
    /// propagation its tools reach the approval gate as `Unknown` and every
    /// external-effect call is refused — the parent's label silently lost at
    /// the `tokio::spawn` boundary.
    #[tokio::test]
    async fn propagate_carries_the_origin_across_a_spawn() {
        let observed = with_origin(
            AgentTurnOrigin::TrustedAutomation {
                job_id: "run-1".to_string(),
                source: TrustedAutomationSource::Workflow {
                    require_approval: false,
                },
            },
            async {
                tokio::spawn(propagate(async { current() }))
                    .await
                    .expect("spawned task panicked")
            },
        )
        .await;
        assert!(matches!(
            observed,
            Some(AgentTurnOrigin::TrustedAutomation {
                source: TrustedAutomationSource::Workflow {
                    require_approval: false
                },
                ..
            })
        ));
    }

    /// Without propagation the same spawn loses the label — the behaviour the
    /// helper above exists to fix, pinned so a future refactor cannot quietly
    /// reintroduce it by dropping the wrapper.
    #[tokio::test]
    async fn a_bare_spawn_loses_the_origin() {
        let observed = with_origin(AgentTurnOrigin::Cli, async {
            tokio::spawn(async { current() })
                .await
                .expect("spawned task panicked")
        })
        .await;
        assert!(observed.is_none());
    }

    /// Fail-closed is preserved: propagation carries a decision, it does not
    /// invent one. An unlabelled parent still yields an unlabelled child.
    #[tokio::test]
    async fn propagate_does_not_manufacture_an_origin() {
        let observed = tokio::spawn(propagate(async { current() }))
            .await
            .expect("spawned task panicked");
        assert!(observed.is_none());
    }

    /// The `hosted::orchestration::effect_executor::run_local_agent` spawn site
    /// (#5508 / #5499): the device-tool bridge fires the local sub-agent from a
    /// bare `tokio::spawn` where there is **no ambient turn** to inherit — unlike
    /// the four sites PR #5465 fixed with `capture`/`propagate`, `capture()` here
    /// is `None`, so `with_inherited_origin` would leave the task `Unknown` and
    /// the gate would refuse every external-effect tool (`cron_add`, shell, …).
    /// The fix scopes an **explicit** `Cli` origin on the spawned task instead
    /// (device automation past the Master-chat gate is trusted, turn-less
    /// internal dispatch). This pins that shape: nothing to inherit on the
    /// parent, a real `Cli` origin observed across the spawn boundary.
    #[tokio::test]
    async fn explicit_cli_origin_survives_a_turnless_spawn() {
        // No outer scope — exactly the device-tool bridge's situation.
        assert!(
            capture().is_none(),
            "precondition: the effect_executor spawn has no ambient origin to inherit"
        );

        let observed = tokio::spawn(with_origin(AgentTurnOrigin::Cli, async { current() }))
            .await
            .expect("spawned task panicked");

        assert!(
            matches!(observed, Some(AgentTurnOrigin::Cli)),
            "the explicitly-scoped Cli origin must be visible on the spawned task, got {observed:?}"
        );
    }

    // ── spawn / spawn_unlabelled ────────────────────────────────────────

    /// The helper carries the label with no separate capture step, so a call
    /// site cannot forget one.
    #[tokio::test]
    async fn spawn_carries_the_origin_onto_the_new_task() {
        let observed = with_origin(AgentTurnOrigin::Cli, async {
            spawn(async { current() })
                .await
                .expect("spawned task panicked")
        })
        .await;

        assert!(
            matches!(observed, Some(AgentTurnOrigin::Cli)),
            "expected the parent's Cli origin on the spawned task, got {observed:?}"
        );
    }

    /// **The reason this helper exists.**
    ///
    /// `propagate` reads the origin when it is *called*, so it has to be called
    /// on the spawning task. Both forms below compile and neither warns, but
    /// evaluating `propagate` inside the spawned future captures nothing — the
    /// task-local is already gone — and the child silently runs unlabelled.
    /// Every external-effect tool it calls is then refused by the approval gate.
    ///
    /// Routing through `spawn` makes that ordering unexpressible.
    #[tokio::test]
    async fn spawn_is_immune_to_capturing_inside_the_spawned_task() {
        let (wrong, right) = with_origin(AgentTurnOrigin::Cli, async {
            // The mistake: `propagate` evaluated on the *new* task.
            let wrong = tokio::spawn(async move { propagate(async { current() }).await })
                .await
                .expect("spawned task panicked");

            // The helper: capture happens before the spawn, inside `spawn`.
            let right = spawn(async { current() })
                .await
                .expect("spawned task panicked");

            (wrong, right)
        })
        .await;

        assert!(
            wrong.is_none(),
            "precondition: capturing inside the spawned task loses the origin — \
             this is the hazard `spawn` removes, got {wrong:?}"
        );
        assert!(
            matches!(right, Some(AgentTurnOrigin::Cli)),
            "the helper must keep the label regardless of how the call site is \
             written, got {right:?}"
        );
    }

    /// Fail-closed: no ambient origin in, no origin out. The helper carries a
    /// decision, it never invents one.
    #[tokio::test]
    async fn spawn_does_not_manufacture_an_origin() {
        assert!(current().is_none(), "test precondition: no ambient scope");

        let observed = spawn(async { current() })
            .await
            .expect("spawned task panicked");

        assert!(
            observed.is_none(),
            "an unlabelled parent must produce an unlabelled child, got {observed:?}"
        );
    }

    /// A remote-untrusted origin crosses as itself. Delegation must not be a
    /// privilege-escalation primitive, so no upgrade to `Cli` may happen.
    #[tokio::test]
    async fn spawn_preserves_an_untrusted_origin_verbatim() {
        let observed = with_origin(
            AgentTurnOrigin::ExternalChannel {
                channel: "telegram".into(),
                sender: Some("u-42".into()),
                reply_target: "chat-7".into(),
                message_id: "m-9".into(),
            },
            async {
                spawn(async { current() })
                    .await
                    .expect("spawned task panicked")
            },
        )
        .await;

        match observed {
            Some(AgentTurnOrigin::ExternalChannel {
                channel, sender, ..
            }) => {
                assert_eq!(channel, "telegram");
                assert_eq!(sender.as_deref(), Some("u-42"));
            }
            other => panic!("expected ExternalChannel carried verbatim, got {other:?}"),
        }
    }

    /// The explicit opt-out drops the label, which is its whole purpose — the
    /// value is that the call site says so by name instead of looking identical
    /// to a site that forgot.
    #[tokio::test]
    async fn spawn_unlabelled_drops_the_origin_on_purpose() {
        let observed = with_origin(AgentTurnOrigin::Cli, async {
            spawn_unlabelled("test: not a continuation of this turn", async { current() })
                .await
                .expect("spawned task panicked")
        })
        .await;

        assert!(
            observed.is_none(),
            "spawn_unlabelled must not carry the caller's origin, got {observed:?}"
        );
    }

    #[tokio::test]
    async fn current_returns_none_outside_scope() {
        assert!(current().is_none());
    }

    #[tokio::test]
    async fn current_returns_inner_origin_on_nested_scope() {
        let observed = with_origin(
            AgentTurnOrigin::WebChat {
                thread_id: "outer".into(),
                client_id: "c-outer".into(),
                request_id: Some("req-outer".into()),
            },
            async {
                with_origin(
                    AgentTurnOrigin::TrustedAutomation {
                        job_id: "j-1".into(),
                        source: TrustedAutomationSource::Cron,
                    },
                    async { current() },
                )
                .await
            },
        )
        .await;
        match observed {
            Some(AgentTurnOrigin::TrustedAutomation { job_id, source }) => {
                assert_eq!(job_id, "j-1");
                assert_eq!(source, TrustedAutomationSource::Cron);
            }
            other => panic!("expected inner TrustedAutomation, got {other:?}"),
        }
    }
}
