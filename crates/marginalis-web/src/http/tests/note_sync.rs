use super::*;

#[tokio::test]
async fn synchronization_uses_a_dedicated_oauth_protected_rest_endpoint() {
    let response = mcp_app()
        .oneshot(
            Request::get("/api/v3/sync/notes?limit=100")
                .header(header::AUTHORIZATION, "Bearer sync-token")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body");
    let page: marginalis_contract::NoteSyncPageResponse =
        serde_json::from_slice(&body).expect("typed synchronization page");
    assert_eq!(
        page.phase,
        marginalis_contract::NoteSyncPhaseResponse::Snapshot
    );
    assert_eq!(page.next_cursor, "next-sync-cursor");
    assert!(!page.has_more);
}

#[tokio::test]
async fn synchronization_requires_the_notes_sync_scope() {
    let response = mcp_app()
        .oneshot(
            Request::get("/api/v3/sync/notes")
                .header(header::AUTHORIZATION, "Bearer read-token")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert!(
        response
            .headers()
            .get(header::WWW_AUTHENTICATE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| {
                value.contains("error=\"insufficient_scope\"")
                    && value.contains("scope=\"notes:read notes:sync\"")
            })
    );
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body");
    let problem: marginalis_contract::ProblemResponse =
        serde_json::from_slice(&body).expect("problem response");
    assert_eq!(problem.code, marginalis_contract::ProblemCode::Forbidden);
}

#[tokio::test]
async fn synchronization_rejects_missing_and_malformed_tokens() {
    for authorization in [None, Some("Basic credentials"), Some("Bearer")] {
        let mut request = Request::get("/api/v3/sync/notes");
        if let Some(authorization) = authorization {
            request = request.header(header::AUTHORIZATION, authorization);
        }
        let response = mcp_app()
            .oneshot(request.body(Body::empty()).expect("request"))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert!(response.headers().contains_key(header::WWW_AUTHENTICATE));
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        let problem: marginalis_contract::ProblemResponse =
            serde_json::from_slice(&body).expect("problem response");
        assert_eq!(
            problem.code,
            marginalis_contract::ProblemCode::AuthenticationRequired
        );
    }
}

#[tokio::test]
async fn synchronization_route_is_stable_when_oauth_is_disabled() {
    let response = app()
        .oneshot(
            Request::get("/api/v3/sync/notes")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}
