use super::*;

fn answer(outcome: ProfileClaim, path: Option<&str>) -> ClaimProfileResponse {
    ClaimProfileResponse {
        outcome,
        path: path.map(str::to_owned),
    }
}

#[test]
fn waits_match_the_task_and_session_policy() {
    assert_eq!(TASK_WAIT_MS, 5_000);
    assert_eq!(SESSION_WAIT_MS, 1_000);
}

#[test]
fn the_request_carries_the_configured_path_and_never_asks_for_a_fresh_folder() {
    let request = claim_request("/home/asha/.openhuman/chrome", TASK_WAIT_MS);
    assert_eq!(request.path, "/home/asha/.openhuman/chrome");
    assert_eq!(request.wait_ms, 5_000);
    assert!(!request.fall_back_to_fresh);
}

#[test]
fn a_claimed_folder_is_kept_and_a_held_one_means_a_fresh_profile() {
    assert_eq!(
        settle("/p", &answer(ProfileClaim::Claimed, Some("/p"))),
        Claim::Claimed("/p".into())
    );
    assert_eq!(
        settle("/p", &answer(ProfileClaim::Claimed, None)),
        Claim::Claimed("/p".into())
    );
    assert_eq!(
        settle("/p", &answer(ProfileClaim::Busy, None)),
        Claim::Fresh
    );
    assert_eq!(
        settle("/p", &answer(ProfileClaim::FellBackToFresh, Some("/tmp/x"))),
        Claim::Fresh
    );
}

#[test]
fn profile_claim_rejects_a_tinycomputer_envelope_version_mismatch() {
    let version_mismatch = DesktopResponse {
        version: "999".into(),
        ..DesktopResponse::ok(
            "browser-claim-profile",
            serde_json::json!({"outcome":"claimed"}),
        )
    };
    assert!(unwrap_response(serde_json::to_value(version_mismatch).unwrap()).is_none());
}

#[test]
fn profile_claim_rejects_success_envelopes_that_also_carry_an_error() {
    let contradictory = DesktopResponse {
        error: Some(tinycomputer_bus::DesktopError::new(
            "BROKEN",
            "contradictory",
        )),
        ..DesktopResponse::ok(
            "browser-claim-profile",
            serde_json::json!({"outcome":"claimed"}),
        )
    };
    assert!(unwrap_response(serde_json::to_value(contradictory).unwrap()).is_none());
}

#[test]
fn profile_claim_keeps_valid_success_and_module_failure_behavior() {
    let success = DesktopResponse::ok(
        "browser-claim-profile",
        serde_json::json!({"outcome":"claimed"}),
    );
    assert_eq!(
        unwrap_response(serde_json::to_value(success).unwrap()).unwrap(),
        serde_json::json!({"outcome":"claimed"})
    );
    let failure = DesktopResponse::err(
        "browser-claim-profile",
        tinycomputer_bus::DesktopError::new("PROFILE_BUSY", "profile is held"),
    );
    assert!(unwrap_response(serde_json::to_value(failure).unwrap()).is_none());
}
