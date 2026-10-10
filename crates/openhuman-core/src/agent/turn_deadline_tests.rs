use super::*;

#[test]
fn default_deadlines_are_780s_840s_900s() {
    let deadline = TurnDeadline::starting_now(Duration::from_secs(DEFAULT_TURN_BACKSTOP_SECS));
    assert_eq!(deadline.wind_down_after(), Duration::from_secs(780));
    assert_eq!(deadline.hard_stop_after(), Duration::from_secs(840));
    assert_eq!(deadline.backstop(), Duration::from_secs(900));
}

/// The ordering production was missing: the harness must stop (and before
/// that wind down) strictly before the backstop drops the turn, for every
/// backstop an operator can configure.
#[test]
fn wind_down_precedes_hard_stop_precedes_backstop_for_every_backstop() {
    for secs in [
        1_u64, 2, 5, 10, 30, 60, 120, 300, 599, 600, 601, 900, 1_200, 3_600, 86_400,
    ] {
        let deadline = TurnDeadline::starting_now(Duration::from_secs(secs));
        assert!(
            deadline.wind_down_after() < deadline.hard_stop_after(),
            "backstop {secs}s: wind-down {:?} must precede hard stop {:?}",
            deadline.wind_down_after(),
            deadline.hard_stop_after()
        );
        assert!(
            deadline.hard_stop_after() < deadline.backstop(),
            "backstop {secs}s: hard stop {:?} must precede backstop",
            deadline.hard_stop_after()
        );
    }
}

/// The harness ceiling (default 3600s) is always clamped below the backstop,
/// and so is an explicit opt-out (`None`).
#[test]
fn harness_wall_clock_is_always_clamped_below_the_backstop() {
    let backstop = Duration::from_secs(DEFAULT_TURN_BACKSTOP_SECS);
    let now = Instant::now();
    let deadline = TurnDeadline::new(now, backstop);
    let backstop_ms = backstop.as_millis() as u64;
    // The harness's default turn ceiling (`DEFAULT_AGENT_TURN_TIMEOUT_SECS`);
    // `deadline_wind_down_tests.rs` pins that constant against this module.
    let harness_default_ms = 3_600_000;

    let clamped = deadline
        .clamp_wall_clock_ms(Some(harness_default_ms), now)
        .expect("clamped ceiling");
    assert_eq!(clamped, 840_000);
    assert!(clamped < backstop_ms);

    let unbounded = deadline.clamp_wall_clock_ms(None, now).expect("bounded");
    assert_eq!(unbounded, 840_000);

    // A configured ceiling that is already tighter is kept.
    assert_eq!(
        deadline.clamp_wall_clock_ms(Some(10_000), now),
        Some(10_000)
    );
}

#[test]
fn clamp_measures_the_remaining_budget_from_the_backstop_start() {
    let now = Instant::now();
    let started = now - Duration::from_secs(800);
    let deadline = TurnDeadline::new(started, Duration::from_secs(900));
    assert_eq!(
        deadline.clamp_wall_clock_ms(Some(3_600_000), now),
        Some(40_000)
    );
    // Past the hard stop the ceiling floors at 1 ms rather than 0.
    let late = TurnDeadline::new(now - Duration::from_secs(899), Duration::from_secs(900));
    assert_eq!(late.clamp_wall_clock_ms(Some(3_600_000), now), Some(1));
}

#[test]
fn should_wind_down_flips_at_the_wind_down_point() {
    let now = Instant::now();
    let fresh = TurnDeadline::new(now - Duration::from_secs(779), Duration::from_secs(900));
    assert!(!fresh.should_wind_down(now));
    let due = TurnDeadline::new(now - Duration::from_secs(780), Duration::from_secs(900));
    assert!(due.should_wind_down(now));
}

#[test]
fn parse_backstop_defaults_and_disables() {
    assert_eq!(parse_backstop(None), Some(Duration::from_secs(900)));
    assert_eq!(
        parse_backstop(Some("garbage")),
        Some(Duration::from_secs(900))
    );
    assert_eq!(
        parse_backstop(Some(" 120 ")),
        Some(Duration::from_secs(120))
    );
    assert_eq!(parse_backstop(Some("0")), None);
}

#[tokio::test]
async fn task_local_scope_is_visible_inside_and_absent_outside() {
    assert!(current().is_none());
    let deadline = TurnDeadline::starting_now(Duration::from_secs(60));
    let seen = with_turn_deadline(Some(deadline), async { current() }).await;
    assert_eq!(seen, Some(deadline));
    assert!(current().is_none());
    assert!(with_turn_deadline(None, async { current() })
        .await
        .is_none());
}
