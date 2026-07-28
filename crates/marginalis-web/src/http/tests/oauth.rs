#[tokio::test]
async fn oauth_consent_uses_session_bound_csrf_when_client_context_has_an_opaque_origin() {
    let state = ApiState::new(
        Arc::new(Notes),
        Arc::new(ActiveSessions),
        Arc::new(Oidc),
        "/".into(),
        "https://example.test".into(),
    );
    let mut headers = HeaderMap::new();
    headers.insert(
        header::COOKIE,
        "marginalis_session=active-session; marginalis_csrf=session-csrf"
            .parse()
            .expect("cookies"),
    );
    headers.insert(header::ORIGIN, "null".parse().expect("opaque origin"));
    headers.insert("sec-fetch-site", "cross-site".parse().expect("metadata"));

    assert!(
        authenticated_form_actor(&headers, &state, "session-csrf")
            .await
            .is_ok()
    );
    assert!(
        authenticated_form_actor(&headers, &state, "forged")
            .await
            .is_err()
    );

    headers.insert(
        header::ORIGIN,
        "https://evil.example".parse().expect("foreign origin"),
    );
    assert!(
        authenticated_form_actor(&headers, &state, "session-csrf")
            .await
            .is_err()
    );
}

#[tokio::test]
async fn mcp_dynamic_registration_creates_a_public_client() {
    let response = mcp_app()
            .oneshot(
                Request::post("/oauth/register")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"client_name":"Claude Code","redirect_uris":["http://localhost:48123/callback"]}"#,
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");
    assert_eq!(response.status(), StatusCode::CREATED);
}

#[tokio::test]
async fn mcp_registration_reports_invalid_redirect_uri() {
    let response = mcp_app()
            .oneshot(
                Request::post("/oauth/register")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"client_name":"Invalid","redirect_uris":["http://remote.example/callback"]}"#,
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    let error: serde_json::Value = serde_json::from_slice(&body).expect("OAuth error");
    assert_eq!(error["error"], "invalid_redirect_uri");
}

#[tokio::test]
async fn mcp_token_response_is_not_cacheable() {
    let response = mcp_app()
            .oneshot(
                Request::post("/oauth/token")
                    .header("content-type", "application/x-www-form-urlencoded")
                    .body(Body::from(
                        "grant_type=authorization_code&code=code&client_id=client&redirect_uri=http%3A%2F%2F127.0.0.1%2Fcallback&resource=https%3A%2F%2Fexample.test%2Fmcp&code_verifier=verifier",
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(header::CACHE_CONTROL),
        Some(&"no-store".parse().expect("header"))
    );
    assert_eq!(
        response.headers().get(header::PRAGMA),
        Some(&"no-cache".parse().expect("header"))
    );
}

#[tokio::test]
async fn mcp_token_errors_use_oauth_error_shape() {
    let response = mcp_app()
            .oneshot(
                Request::post("/oauth/token")
                    .header("content-type", "application/x-www-form-urlencoded")
                    .body(Body::from(
                        "grant_type=password&client_id=client&resource=https%3A%2F%2Fexample.test%2Fmcp",
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        response.headers().get(header::CACHE_CONTROL),
        Some(&"no-store".parse().expect("header"))
    );
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    let error: serde_json::Value = serde_json::from_slice(&body).expect("OAuth error");
    assert_eq!(error["error"], "unsupported_grant_type");

    let client_authentication = mcp_app()
            .oneshot(
                Request::post("/oauth/token")
                    .header(header::AUTHORIZATION, "Basic ZHVtbXk6ZHVtbXk=")
                    .body(Body::from(
                        "grant_type=authorization_code&code=code&client_id=client&resource=https%3A%2F%2Fexample.test%2Fmcp&code_verifier=verifier",
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");
    assert_eq!(client_authentication.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        client_authentication
            .headers()
            .get(header::WWW_AUTHENTICATE),
        Some(&"Basic".parse().expect("challenge"))
    );
    let body = axum::body::to_bytes(client_authentication.into_body(), usize::MAX)
        .await
        .expect("body");
    let error: serde_json::Value = serde_json::from_slice(&body).expect("OAuth error");
    assert_eq!(error["error"], "invalid_client");
}

#[tokio::test]
async fn mcp_token_accepts_an_omitted_redirect_and_rejects_duplicate_parameters() {
    let omitted_redirect = mcp_app()
            .oneshot(
                Request::post("/oauth/token")
                    .header("content-type", "application/x-www-form-urlencoded")
                    .body(Body::from(
                        "grant_type=authorization_code&code=code&client_id=client&resource=https%3A%2F%2Fexample.test%2Fmcp&code_verifier=verifier",
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");
    assert_eq!(omitted_redirect.status(), StatusCode::OK);

    let duplicate = mcp_app()
            .oneshot(
                Request::post("/oauth/token")
                    .header("content-type", "application/x-www-form-urlencoded")
                    .body(Body::from(
                        "grant_type=authorization_code&grant_type=refresh_token&client_id=client&resource=https%3A%2F%2Fexample.test%2Fmcp",
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");
    assert_eq!(duplicate.status(), StatusCode::BAD_REQUEST);
    let body = axum::body::to_bytes(duplicate.into_body(), usize::MAX)
        .await
        .expect("body");
    let error: serde_json::Value = serde_json::from_slice(&body).expect("OAuth error");
    assert_eq!(error["error"], "invalid_request");

    let downscoped = mcp_app()
            .oneshot(
                Request::post("/oauth/token")
                    .header("content-type", "application/x-www-form-urlencoded")
                    .body(Body::from(
                        "grant_type=refresh_token&refresh_token=refresh-ok&client_id=client&resource=https%3A%2F%2Fexample.test%2Fmcp&scope=notes%3Aread",
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");
    assert_eq!(downscoped.status(), StatusCode::OK);
    let body = axum::body::to_bytes(downscoped.into_body(), usize::MAX)
        .await
        .expect("body");
    let token: serde_json::Value = serde_json::from_slice(&body).expect("token");
    assert_eq!(token["scope"], "notes:read");
}

#[tokio::test]
async fn public_mcp_endpoints_reject_oversized_request_bodies() {
    let registration = mcp_app()
        .oneshot(
            Request::post("/oauth/register")
                .header("content-type", "application/json")
                .body(Body::from(vec![b' '; 16 * 1024 + 1]))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(registration.status(), StatusCode::PAYLOAD_TOO_LARGE);

    let mcp = mcp_app()
        .oneshot(
            Request::post("/mcp")
                .header("content-type", "application/json")
                .body(Body::from(vec![b' '; 1024 * 1024 + 1]))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(mcp.status(), StatusCode::PAYLOAD_TOO_LARGE);
}
