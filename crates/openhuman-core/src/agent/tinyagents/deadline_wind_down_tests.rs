use super::*;
use std::time::Duration;

fn handle() -> SteeringHandle {
    crate::agent::tinyagents::host::steering::openhuman_steering_handle(
        crate::agent::tinyagents::host::steering::SteeringRunClass::Interactive,
    )
}

#[test]
fn does_not_pause_before_the_wind_down_point() {
    let now = Instant::now();
    let deadline = TurnDeadline::new(now - Duration::from_secs(100), Duration::from_secs(900));
    let mw = DeadlineWindDownMiddleware::new(handle(), deadline);
    assert!(!mw.maybe_pause("after_model", now));
    assert!(!mw.fired());
}

#[test]
fn pauses_once_after_the_wind_down_point() {
    let now = Instant::now();
    let deadline = TurnDeadline::new(now - Duration::from_secs(781), Duration::from_secs(900));
    let mw = DeadlineWindDownMiddleware::new(handle(), deadline);
    assert!(
        mw.maybe_pause("after_tool", now),
        "first checkpoint past wind-down pauses"
    );
    assert!(mw.fired());
    assert!(
        !mw.maybe_pause("after_model", now),
        "the pause is latched and sent exactly once"
    );
}

/// The harness's own default turn ceiling is later than the web backstop.
/// That ordering is what dropped long turns. The clamp must therefore always
/// bring it below the backstop.
#[test]
fn the_default_harness_ceiling_is_clamped_below_the_backstop() {
    let backstop = Duration::from_secs(crate::agent::turn_deadline::DEFAULT_TURN_BACKSTOP_SECS);
    let default_ms = crate::agent::tinyagents::DEFAULT_AGENT_TURN_TIMEOUT_SECS * 1_000;
    assert!(
        default_ms > backstop.as_millis() as u64,
        "precondition: the unclamped ceiling is later than the backstop"
    );
    let now = Instant::now();
    let clamped = TurnDeadline::new(now, backstop)
        .clamp_wall_clock_ms(Some(default_ms), now)
        .expect("clamped");
    assert!(clamped < backstop.as_millis() as u64);
}

#[test]
fn install_clamps_the_policy_and_adds_the_middleware_for_a_root_turn() {
    let mut harness: AgentHarness<(), OpenHumanRunContext> = AgentHarness::new();
    harness.with_policy(crate::agent::tinyagents::run_policy_for(200, false));
    let now = Instant::now();
    let deadline = TurnDeadline::new(now, Duration::from_secs(900));
    let h = handle();
    let mw = install_for(&mut harness, Some(&h), Some(deadline), false);
    assert!(mw.is_some());
    let clamped = harness
        .policy()
        .limits
        .max_wall_clock_ms
        .expect("bounded wall clock");
    assert!(
        clamped <= 840_000,
        "clamped to the hard stop, got {clamped}"
    );
    assert!(
        clamped > 830_000,
        "measured from the deadline start, got {clamped}"
    );
}

#[test]
fn install_leaves_children_and_unbounded_turns_alone() {
    let h = handle();
    let deadline = TurnDeadline::starting_now(Duration::from_secs(900));

    let mut child: AgentHarness<(), OpenHumanRunContext> = AgentHarness::new();
    child.with_policy(crate::agent::tinyagents::run_policy_for(10, false));
    let before = child.policy().limits.max_wall_clock_ms;
    assert!(install_for(&mut child, Some(&h), Some(deadline), true).is_none());
    assert_eq!(child.policy().limits.max_wall_clock_ms, before);

    let mut plain: AgentHarness<(), OpenHumanRunContext> = AgentHarness::new();
    plain.with_policy(crate::agent::tinyagents::run_policy_for(10, false));
    let before = plain.policy().limits.max_wall_clock_ms;
    assert!(install_for(&mut plain, Some(&h), None, false).is_none());
    assert_eq!(plain.policy().limits.max_wall_clock_ms, before);
}

/// The wind-down pause is how the turn ended, so it must reach the session
/// sidecar the driver classifies the turn's stop from. Before, the middleware
/// only paused, and a wound-down turn was traced as a clean completion.
#[test]
fn a_wind_down_pause_is_recorded_on_the_turn_sidecar() {
    let mut harness: AgentHarness<(), OpenHumanRunContext> = AgentHarness::new();
    harness.with_policy(crate::agent::tinyagents::run_policy_for(200, false));
    let now = Instant::now();
    let mut run_context = OpenHumanRunContext::new();
    run_context.turn_deadline = Some(TurnDeadline::new(
        now - Duration::from_secs(781),
        Duration::from_secs(900),
    ));
    let mw = install(&mut harness, &Some(handle()), &run_context, &None).expect("installed");
    assert!(!run_context.session_sidecar.lock().unwrap().wind_down);
    assert!(mw.maybe_pause("after_tool", Instant::now()));
    assert!(
        run_context.session_sidecar.lock().unwrap().wind_down,
        "the pause must be visible to the driver"
    );
}
