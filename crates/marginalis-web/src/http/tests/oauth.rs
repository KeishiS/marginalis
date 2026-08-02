use super::*;
use crate::http::auth::authenticated_form_actor;

#[tokio::test]
async fn oauth_endpoints_are_not_exposed_when_mcp_is_disabled() {
    for path in [
        "/.well-known/oauth-authorization-server",
        "/oauth/authorize",
        "/oauth/register",
        "/oauth/token",
        "/oauth/revoke",
    ] {
        let response = app()
            .oneshot(Request::get(path).body(Body::empty()).expect("request"))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
    }
}

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
async fn authorization_error_response_identifies_the_issuer() {
    let response = mcp_app()
        .oneshot(
            Request::get("/oauth/authorize?response_type=token&client_id=client&redirect_uri=https%3A%2F%2Fclient.example.test%2Fcallback&state=opaque-state")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    let location = response
        .headers()
        .get(header::LOCATION)
        .and_then(|value| value.to_str().ok())
        .expect("redirect location");
    let location = url::Url::parse(location).expect("redirect URL");
    let parameters = location
        .query_pairs()
        .collect::<std::collections::HashMap<_, _>>();
    assert_eq!(
        parameters.get("error").map(|value| value.as_ref()),
        Some("unsupported_response_type")
    );
    assert_eq!(
        parameters.get("state").map(|value| value.as_ref()),
        Some("opaque-state")
    );
    assert_eq!(
        parameters.get("iss").map(|value| value.as_ref()),
        Some("https://example.test/")
    );
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

/// metadataの各endpointは、サブパスを含む公開base URLから同じ規則で導く。
#[tokio::test]
async fn authorization_server_metadata_derives_every_endpoint_from_the_base_url() {
    let app = TestApp::default()
        .mcp(
            "https://example.test/marginalis",
            Vec::new(),
            Arc::new(TestMcpAuthenticator),
        )
        .router();
    let response = app
        .oneshot(
            Request::get("/.well-known/oauth-authorization-server/marginalis")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    let metadata: serde_json::Value = serde_json::from_slice(&body).expect("metadata");
    assert_eq!(metadata["issuer"], "https://example.test/marginalis");
    assert_eq!(
        metadata["authorization_endpoint"],
        "https://example.test/marginalis/oauth/authorize"
    );
    assert_eq!(
        metadata["token_endpoint"],
        "https://example.test/marginalis/oauth/token"
    );
    assert_eq!(
        metadata["revocation_endpoint"],
        "https://example.test/marginalis/oauth/revoke"
    );
    assert_eq!(
        metadata["registration_endpoint"],
        "https://example.test/marginalis/oauth/register"
    );
    assert_eq!(
        metadata["protected_resources"][0],
        "https://example.test/marginalis/mcp"
    );
}

fn revoke_authorization_request(client_id: &str) -> axum::http::request::Builder {
    Request::builder()
        .method("DELETE")
        .uri(format!("/api/v3/mcp-authorizations/{client_id}"))
        .header(header::ORIGIN, "https://example.test")
        .header("sec-fetch-site", "same-origin")
        .header(
            header::COOKIE,
            "marginalis_session=active-session; marginalis_csrf=session-csrf",
        )
}

/// Web UIから接続を取り消すRESTは、他の変更操作と同じ認証・CSRF検査を通す。
#[tokio::test]
async fn revoking_an_mcp_authorization_requires_an_authenticated_same_origin_request() {
    let response = authenticated_mcp_app()
        .oneshot(
            revoke_authorization_request("mcp-0197c9bc-0000-7000-8000-000000000001")
                .header("x-csrf-token", "session-csrf")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let response = authenticated_mcp_app()
        .oneshot(
            revoke_authorization_request("mcp-0197c9bc-0000-7000-8000-000000000001")
                .header("x-csrf-token", "forged")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    let response = mcp_app()
        .oneshot(
            revoke_authorization_request("mcp-0197c9bc-0000-7000-8000-000000000001")
                .header("x-csrf-token", "session-csrf")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

/// 経路のclient IDがそのまま業務処理へ渡ることを、応答の違いで確認する。
#[tokio::test]
async fn revoking_an_mcp_authorization_passes_the_client_id_through() {
    let response = authenticated_mcp_app()
        .oneshot(
            revoke_authorization_request("unavailable-client")
                .header("x-csrf-token", "session-csrf")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn revoking_an_mcp_authorization_accepts_an_encoded_metadata_document_client_id() {
    let response = authenticated_mcp_app()
        .oneshot(
            revoke_authorization_request("https%3A%2F%2Fclient.example.test%2Fmetadata.json")
                .header("x-csrf-token", "session-csrf")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn revoking_an_mcp_authorization_rejects_an_oversized_client_id() {
    let response = authenticated_mcp_app()
        .oneshot(
            revoke_authorization_request(&"a".repeat(2_049))
                .header("x-csrf-token", "session-csrf")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

/// MCPが無効な構成でも公開RESTの形は変えず、利用できないことを伝える。
#[tokio::test]
async fn revoking_an_mcp_authorization_reports_that_mcp_is_unavailable() {
    let response = TestApp::default()
        .authenticated()
        .router()
        .oneshot(
            revoke_authorization_request("mcp-0197c9bc-0000-7000-8000-000000000001")
                .header("x-csrf-token", "session-csrf")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

/// 保存上限は要求の誤りではないため、client metadataの拒否と区別して伝える。
#[tokio::test]
async fn mcp_registration_reports_capacity_as_temporarily_unavailable() {
    let response = mcp_app()
            .oneshot(
                Request::post("/oauth/register")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"client_name":"At capacity","redirect_uris":["https://at-capacity.example/callback"]}"#,
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    let error: serde_json::Value = serde_json::from_slice(&body).expect("OAuth error");
    assert_eq!(error["error"], "temporarily_unavailable");
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
async fn rfc7009_revocation_is_exposed_for_public_clients() {
    let response = mcp_app()
        .oneshot(
            Request::post("/oauth/revoke")
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from("token=opaque-token&client_id=public-client"))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(header::CACHE_CONTROL),
        Some(&"no-store".parse().expect("header"))
    );
}

#[tokio::test]
async fn mcp_token_requires_redirect_and_rejects_duplicate_parameters() {
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
    assert_eq!(omitted_redirect.status(), StatusCode::BAD_REQUEST);

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
