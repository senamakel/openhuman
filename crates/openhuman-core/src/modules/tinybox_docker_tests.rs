//! Focused Docker transport and deadline tests for TinyBox.

use super::tests::{mock_client, mock_client_with_docker_control_delay};
use super::*;
use serde_json::json;

#[test]
fn resolve_local_grants_is_a_member_of_the_canonical_contract() {
    assert_eq!(RESOLVE_LOCAL_GRANTS, "ResolveLocalGrants");
    assert!(contract::METHODS.contains(&RESOLVE_LOCAL_GRANTS));
}

#[tokio::test]
async fn docker_controls_use_typed_calls_with_validated_contract_capability(
) -> Result<(), Box<dyn std::error::Error>> {
    let (_service, client, calls) = mock_client().await;
    let cache = CompatibilityCache::new(None);
    let status = call_docker_status_cached(
        &client,
        contract::DockerStatusRequest { timeout_ms: 2_000 },
        &cache,
    )
    .await?;
    assert!(status.available);

    let cleanup = call_cleanup_docker_cached(
        &client,
        contract::CleanupDockerRequest {
            label: "openhuman.sandbox=true".to_owned(),
            timeout_ms: 2_000,
        },
        &cache,
    )
    .await?;
    assert_eq!(cleanup.containers_stopped, 3);

    let calls = calls.lock().expect("recorded TinyBox calls");
    let status_call = calls
        .iter()
        .find(|(member, _)| member == DOCKER_STATUS)
        .expect("status member called");
    assert_eq!(status_call.1, json!([{ "timeout_ms": 2_000 }]));
    let cleanup_call = calls
        .iter()
        .find(|(member, _)| member == CLEANUP_DOCKER)
        .expect("cleanup member called");
    assert_eq!(
        cleanup_call.1,
        json!([{ "label": "openhuman.sandbox=true", "timeout_ms": 2_000 }])
    );
    assert_eq!(
        calls
            .iter()
            .filter(|(member, _)| member == CAPABILITIES)
            .count(),
        1
    );
    Ok(())
}

#[test]
fn docker_control_transport_deadline_tracks_the_bounded_module_operation() {
    assert_eq!(
        docker_control_deadline(10_000, DOCKER_CALL_GRACE),
        std::time::Duration::from_secs(15)
    );
    assert_eq!(
        docker_control_deadline(1, DOCKER_CALL_GRACE),
        std::time::Duration::from_millis(5_001)
    );
    assert_eq!(
        docker_control_deadline(u64::MAX, DOCKER_CALL_GRACE),
        std::time::Duration::from_secs(65),
        "an invalid oversized request must not create an effectively unbounded bus wait"
    );
}

#[tokio::test]
async fn docker_status_transport_uses_its_bounded_deadline() {
    let (_service, client, _) =
        mock_client_with_docker_control_delay(std::time::Duration::from_millis(100)).await;
    let error = call_docker_status_with_grace(
        &client,
        contract::DockerStatusRequest { timeout_ms: 5 },
        std::time::Duration::from_millis(5),
    )
    .await
    .expect_err("transport must stop waiting at operation deadline plus grace");

    assert!(error.contains("MODULE_CALL_REPORTED"), "{error}");

    let error = call_cleanup_docker_with_grace(
        &client,
        contract::CleanupDockerRequest {
            label: "openhuman.sandbox=true".to_owned(),
            timeout_ms: 5,
        },
        std::time::Duration::from_millis(5),
    )
    .await
    .expect_err("cleanup transport must also stop at its bounded deadline plus grace");
    assert!(error.contains("MODULE_CALL_REPORTED"), "{error}");
}
