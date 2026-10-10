use super::*;

#[test]
fn single_user_surfaces_are_closed() {
    for path in [
        "/v1",
        "/v1/chat/completions",
        "/events/domain",
        "/events/webhooks",
        "/ws/dictation",
        "/socket.io/",
        "/dev/connect",
        "/oauth/mcp/callback",
    ] {
        assert!(is_closed_in_saas(path), "{path}");
    }
}

#[test]
fn the_gateway_surfaces_stay_open() {
    // `/events` itself is the per-user chat stream, gated on the user scope by
    // the layer rather than closed by prefix.
    for path in [
        "/",
        "/health",
        "/schema",
        "/rpc",
        "/v1x",
        "/eventsource",
        "/events",
    ] {
        assert!(!is_closed_in_saas(path), "{path}");
    }
}

mod decision {
    use super::*;
    use crate::core_host::profiles::gateway::sign;
    use axum::body::Body;
    use axum::routing::get;
    use axum::Router;
    use tower::ServiceExt;

    const SECRET: &str = "service-token";
    const NOW: u64 = 1_700_000_000;

    /// Stands in for `resolve_scope`, which reads the process's profile host: a
    /// user needs a valid signature, and only `alice` is provisioned.
    fn resolve(
        user: Option<&str>,
        sig: Option<&str>,
        secret: &str,
        now: u64,
    ) -> Result<GatewayScope, GatewayRefusal> {
        let refuse = |status, message: &str| GatewayRefusal::new(status, message);
        let Some(user) = user else {
            return Ok(GatewayScope::Operator);
        };
        let sig = sig.ok_or_else(|| refuse(401, "missing signature"))?;
        crate::core_host::profiles::gateway::verify(secret, user, sig, now)
            .map_err(|e| refuse(401, &e))?;
        if user == "alice" {
            // The real resolver returns the user's profile, which needs a
            // booted host; the operator scope stands in for "accepted".
            Ok(GatewayScope::Operator)
        } else if user == "carol" {
            // Hosted by another node.
            Err(GatewayRefusal {
                status: 409,
                message: crate::core_host::profiles::gateway::PROFILE_HELD.to_string(),
                held_by: Some(crate::core_host::profiles::gateway::HeldBy {
                    owner: "node-a".to_string(),
                    endpoint: Some("http://10.0.0.1:7788".to_string()),
                    retry_after_ms: 1_500,
                }),
            })
        } else {
            Err(refuse(403, "not provisioned"))
        }
    }

    fn app(secret: Option<&'static str>) -> Router {
        Router::new()
            .route("/rpc", get(|| async { "ok" }))
            .route("/events", get(|| async { "ok" }))
            .route("/v1/models", get(|| async { "ok" }))
            .layer(axum::middleware::from_fn(
                move |req: Request, next: Next| async move {
                    match decide(&req, secret) {
                        Ok(Admitted::Operator) => next.run(req).await,
                        Ok(Admitted::User { user, signature }) => {
                            match resolve(Some(&user), signature.as_deref(), SECRET, NOW) {
                                Ok(_) => next.run(req).await,
                                Err(refusal) => refusal_response(refusal),
                            }
                        }
                        Err(refused) => refused.into_response(),
                    }
                },
            ))
    }

    async fn status(app: Router, path: &str, headers: &[(&str, String)]) -> StatusCode {
        let mut req = Request::builder().uri(path);
        for (name, value) in headers {
            req = req.header(*name, value);
        }
        app.oneshot(req.body(Body::empty()).unwrap())
            .await
            .unwrap()
            .status()
    }

    fn bearer_header() -> (&'static str, String) {
        ("authorization", format!("Bearer {SECRET}"))
    }

    fn user(id: &str) -> (&'static str, String) {
        (USER_HEADER, id.to_string())
    }

    fn sig(id: &str, ts: u64) -> (&'static str, String) {
        (USER_SIG_HEADER, sign(SECRET, id, ts))
    }

    #[tokio::test]
    async fn closed_routes_are_404_even_for_a_valid_caller() {
        let headers = [bearer_header(), user("alice"), sig("alice", NOW)];
        assert_eq!(
            status(app(Some(SECRET)), "/v1/models", &headers).await,
            StatusCode::NOT_FOUND
        );
    }

    #[tokio::test]
    async fn no_user_header_runs_on_the_operator_plane() {
        assert_eq!(status(app(Some(SECRET)), "/rpc", &[]).await, StatusCode::OK);
    }

    #[tokio::test]
    async fn the_chat_event_stream_is_not_the_operators() {
        assert_eq!(
            status(app(Some(SECRET)), "/events", &[bearer_header()]).await,
            StatusCode::NOT_FOUND
        );
    }

    #[tokio::test]
    async fn a_user_request_without_the_bearer_is_401_before_anything_else() {
        let app = app(Some(SECRET));
        assert_eq!(
            status(app.clone(), "/rpc", &[user("alice"), sig("alice", NOW)]).await,
            StatusCode::UNAUTHORIZED
        );
        let wrong = ("authorization", "Bearer nope".to_string());
        assert_eq!(
            status(app, "/rpc", &[wrong, user("nobody"), sig("nobody", NOW)]).await,
            StatusCode::UNAUTHORIZED,
            "an unauthenticated caller must not learn who is provisioned"
        );
    }

    #[tokio::test]
    async fn a_user_request_before_the_core_has_a_token_is_503() {
        assert_eq!(
            status(app(None), "/rpc", &[bearer_header(), user("alice")]).await,
            StatusCode::SERVICE_UNAVAILABLE
        );
    }

    #[tokio::test]
    async fn a_valid_signed_user_is_admitted() {
        let headers = [bearer_header(), user("alice"), sig("alice", NOW)];
        assert_eq!(
            status(app(Some(SECRET)), "/rpc", &headers).await,
            StatusCode::OK
        );
    }

    #[tokio::test]
    async fn the_signature_window_is_plus_or_minus_sixty_seconds() {
        for ts in [NOW - 60, NOW + 60] {
            let headers = [bearer_header(), user("alice"), sig("alice", ts)];
            assert_eq!(
                status(app(Some(SECRET)), "/rpc", &headers).await,
                StatusCode::OK,
                "t={ts}"
            );
        }
        for ts in [NOW - 61, NOW + 61] {
            let headers = [bearer_header(), user("alice"), sig("alice", ts)];
            assert_eq!(
                status(app(Some(SECRET)), "/rpc", &headers).await,
                StatusCode::UNAUTHORIZED,
                "t={ts}"
            );
        }
    }

    #[tokio::test]
    async fn a_missing_or_forged_signature_is_refused() {
        let app = app(Some(SECRET));
        assert_eq!(
            status(app.clone(), "/rpc", &[bearer_header(), user("alice")]).await,
            StatusCode::UNAUTHORIZED
        );
        // Signed for another user.
        let headers = [bearer_header(), user("alice"), sig("mallory", NOW)];
        assert_eq!(
            status(app.clone(), "/rpc", &headers).await,
            StatusCode::UNAUTHORIZED
        );
        // Signed with another key.
        let forged = (USER_SIG_HEADER, sign("other-key", "alice", NOW));
        assert_eq!(
            status(app, "/rpc", &[bearer_header(), user("alice"), forged]).await,
            StatusCode::UNAUTHORIZED
        );
    }

    #[tokio::test]
    async fn an_unprovisioned_user_is_403() {
        let headers = [bearer_header(), user("bob"), sig("bob", NOW)];
        assert_eq!(
            status(app(Some(SECRET)), "/rpc", &headers).await,
            StatusCode::FORBIDDEN
        );
    }

    #[tokio::test]
    async fn a_repeated_user_header_is_400() {
        let headers = [
            bearer_header(),
            user("alice"),
            user("bob"),
            sig("alice", NOW),
        ];
        assert_eq!(
            status(app(Some(SECRET)), "/rpc", &headers).await,
            StatusCode::BAD_REQUEST
        );
    }

    #[tokio::test]
    async fn a_repeated_signature_header_is_400_not_first_wins() {
        // The first signature is valid; the second must not be ignored.
        let headers = [
            bearer_header(),
            user("alice"),
            sig("alice", NOW),
            sig("alice", NOW + 1),
        ];
        assert_eq!(
            status(app(Some(SECRET)), "/rpc", &headers).await,
            StatusCode::BAD_REQUEST
        );
    }

    #[tokio::test]
    async fn a_repeated_signature_header_does_not_leak_to_an_unauthenticated_caller() {
        let headers = [user("alice"), sig("alice", NOW), sig("alice", NOW)];
        assert_eq!(
            status(app(Some(SECRET)), "/rpc", &headers).await,
            StatusCode::UNAUTHORIZED
        );
    }

    #[tokio::test]
    async fn a_profile_hosted_elsewhere_is_409_naming_the_owner() {
        let headers = [bearer_header(), user("carol"), sig("carol", NOW)];
        let mut req = Request::builder().uri("/rpc");
        for (name, value) in &headers {
            req = req.header(*name, value);
        }
        let response = app(Some(SECRET))
            .oneshot(req.body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CONFLICT);
        assert_eq!(
            response.headers()[PROFILE_OWNER_HEADER].to_str().unwrap(),
            "node-a"
        );
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            body,
            serde_json::json!({
                "error": "profile_held",
                "owner": "node-a",
                "endpoint": "http://10.0.0.1:7788",
                "retry_after_ms": 1500,
            })
        );
    }

    #[tokio::test]
    async fn a_held_profile_is_not_revealed_to_an_unauthenticated_caller() {
        let headers = [user("carol"), sig("carol", NOW)];
        assert_eq!(
            status(app(Some(SECRET)), "/rpc", &headers).await,
            StatusCode::UNAUTHORIZED
        );
    }
}
