#[tokio::test]
async fn mcp_metadata_is_available_when_enabled() {
    let response = mcp_app()
        .oneshot(
            Request::get("/.well-known/oauth-authorization-server")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("metadata body");
    let metadata: serde_json::Value = serde_json::from_slice(&body).expect("metadata");
    assert_eq!(
        metadata["scopes_supported"],
        serde_json::json!(["notes:read", "notes:write", "notes:delete"])
    );

    let protected = mcp_app()
        .oneshot(
            Request::get("/.well-known/oauth-protected-resource/mcp")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(protected.status(), StatusCode::OK);
    let body = axum::body::to_bytes(protected.into_body(), usize::MAX)
        .await
        .expect("metadata body");
    let metadata: serde_json::Value = serde_json::from_slice(&body).expect("metadata");
    assert_eq!(metadata["resource_name"], "Marginalis MCP");
}

#[tokio::test]
async fn external_authorization_changes_discovery_and_access_token_authentication() {
    let protected = external_mcp_app()
        .oneshot(
            Request::get("/.well-known/oauth-protected-resource/mcp")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(protected.status(), StatusCode::OK);
    let metadata: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(protected.into_body(), usize::MAX)
            .await
            .expect("metadata body"),
    )
    .expect("metadata");
    assert_eq!(
        metadata["authorization_servers"],
        serde_json::json!(["https://evaluation.jp.auth0.com/"])
    );

    let accepted = external_mcp_app()
        .oneshot(
            Request::post("/mcp")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::ACCEPT, "application/json, text/event-stream")
                .header(header::AUTHORIZATION, "Bearer external-token")
                .body(Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#,
                ))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(accepted.status(), StatusCode::OK);

    let internal_token = external_mcp_app()
        .oneshot(
            Request::post("/mcp")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::ACCEPT, "application/json, text/event-stream")
                .header(header::AUTHORIZATION, "Bearer valid-token")
                .body(Body::from(
                    r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
                ))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(internal_token.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn mcp_metadata_uses_rfc_well_known_paths_for_a_subpath_issuer() {
    for path in [
        "/.well-known/oauth-protected-resource/marginalis/mcp",
        "/.well-known/oauth-authorization-server/marginalis",
    ] {
        let response = subpath_mcp_app()
            .oneshot(Request::get(path).body(Body::empty()).expect("request"))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK, "{path}");
    }

    let non_standard = subpath_mcp_app()
        .oneshot(
            Request::get("/marginalis/.well-known/oauth-authorization-server")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(non_standard.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn mcp_authorization_starts_login_when_no_web_session_exists() {
    let response = mcp_app()
            .oneshot(
                Request::get(
                    "/oauth/authorize?response_type=code&client_id=client&redirect_uri=http%3A%2F%2F127.0.0.1%3A48123%2Fcallback&resource=https%3A%2F%2Fexample.test%2Fmcp&scope=notes%3Aread&code_challenge=verifier&code_challenge_method=S256&state=opaque",
                )
                .body(Body::empty())
                .expect("request"),
            )
            .await
            .expect("response");
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    let login_location = response
        .headers()
        .get(header::LOCATION)
        .expect("login location")
        .to_str()
        .expect("valid location")
        .to_owned();
    assert!(login_location.starts_with("/auth/oidc/login?next="));

    let login = mcp_app()
        .oneshot(
            Request::get(login_location)
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(login.status(), StatusCode::TEMPORARY_REDIRECT);
    assert_eq!(
        login.headers().get(header::LOCATION).expect("location"),
        "https://id.example.test/authorize"
    );
    assert!(
        login
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .any(|value| value
                .to_str()
                .is_ok_and(|value| value.contains(RETURN_TO_COOKIE)))
    );
}

#[tokio::test]
async fn cross_origin_oauth_posts_start_login_without_bypassing_consent_csrf() {
    let form_post = mcp_app()
            .oneshot(
                Request::post("/oauth/authorize")
                    .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                    .header(header::ORIGIN, "https://chatgpt.com")
                    .header("sec-fetch-site", "cross-site")
                    .body(Body::from(
                        "response_type=code&client_id=client&redirect_uri=https%3A%2F%2Fchatgpt.com%2Fconnector%2Foauth%2Fcallback&resource=https%3A%2F%2Fexample.test%2Fmcp&scope=notes%3Aread&code_challenge=verifier&code_challenge_method=S256&state=opaque",
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");
    assert_eq!(form_post.status(), StatusCode::SEE_OTHER);
    assert!(
        form_post
            .headers()
            .get(header::LOCATION)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|location| location.starts_with("/auth/oidc/login?next="))
    );

    let query_post = mcp_app()
            .oneshot(
                Request::post(
                    "/oauth/authorize?response_type=code&client_id=client&redirect_uri=https%3A%2F%2Fchatgpt.com%2Fconnector%2Foauth%2Fcallback&resource=https%3A%2F%2Fexample.test%2Fmcp&scope=notes%3Aread&code_challenge=verifier&code_challenge_method=S256&state=opaque",
                )
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .header(header::ORIGIN, "https://chatgpt.com")
                .header("sec-fetch-site", "cross-site")
                .body(Body::from("csrf_token=client-owned-value"))
                .expect("request"),
            )
            .await
            .expect("response");
    assert_eq!(query_post.status(), StatusCode::SEE_OTHER);
    assert!(
        query_post
            .headers()
            .get(header::LOCATION)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|location| location.starts_with("/auth/oidc/login?next="))
    );

    let conflicting_post = mcp_app()
            .oneshot(
                Request::post(
                    "/oauth/authorize?response_type=code&client_id=client&redirect_uri=https%3A%2F%2Fchatgpt.com%2Fconnector%2Foauth%2Fcallback&resource=https%3A%2F%2Fexample.test%2Fmcp&scope=notes%3Aread&code_challenge=verifier&code_challenge_method=S256&state=opaque",
                )
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from("client_id=different-client"))
                .expect("request"),
            )
            .await
            .expect("response");
    assert_eq!(conflicting_post.status(), StatusCode::SEE_OTHER);
    assert!(
        conflicting_post
            .headers()
            .get(header::LOCATION)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|location| location.contains("error=invalid_request"))
    );

    let forged_approval = mcp_app()
            .oneshot(
                Request::post("/oauth/authorize/consent")
                    .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                    .header(header::ORIGIN, "https://chatgpt.com")
                    .header("sec-fetch-site", "cross-site")
                    .body(Body::from(
                        "client_id=client&redirect_uri=https%3A%2F%2Fchatgpt.com%2Fconnector%2Foauth%2Fcallback&resource=https%3A%2F%2Fexample.test%2Fmcp&scope=notes%3Aread&code_challenge=verifier&state=opaque&csrf_token=forged&decision=approve",
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");
    assert_eq!(forged_approval.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn authorization_errors_redirect_only_after_client_redirect_validation() {
    let invalid_target = mcp_app()
            .oneshot(
                Request::get(
                    "/oauth/authorize?response_type=code&client_id=client&redirect_uri=https%3A%2F%2Fchatgpt.com%2Fconnector%2Foauth%2Fcallback&resource=https%3A%2F%2Fother.example%2Fmcp&code_challenge=verifier&code_challenge_method=S256&state=opaque",
                )
                .body(Body::empty())
                .expect("request"),
            )
            .await
            .expect("response");
    assert_eq!(invalid_target.status(), StatusCode::SEE_OTHER);
    let location = invalid_target
        .headers()
        .get(header::LOCATION)
        .and_then(|value| value.to_str().ok())
        .expect("redirect location");
    assert!(location.starts_with("https://chatgpt.com/connector/oauth/callback?"));
    assert!(location.contains("error=invalid_target"));
    assert!(location.contains("state=opaque"));

    let missing_client = mcp_app()
            .oneshot(
                Request::get(
                    "/oauth/authorize?response_type=code&redirect_uri=https%3A%2F%2Fevil.example%2Fcallback",
                )
                .body(Body::empty())
                .expect("request"),
            )
            .await
            .expect("response");
    assert_eq!(missing_client.status(), StatusCode::BAD_REQUEST);
    assert!(!missing_client.headers().contains_key(header::LOCATION));

    let oversized_state = "x".repeat(3_000);
    let oversized_resume = mcp_app()
            .oneshot(
                Request::get(format!(
                    "/oauth/authorize?response_type=code&client_id=client&redirect_uri=https%3A%2F%2Fchatgpt.com%2Fconnector%2Foauth%2Fcallback&resource=https%3A%2F%2Fexample.test%2Fmcp&scope=notes%3Aread&code_challenge=verifier&code_challenge_method=S256&state={oversized_state}"
                ))
                .body(Body::empty())
                .expect("request"),
            )
            .await
            .expect("response");
    assert_eq!(oversized_resume.status(), StatusCode::SEE_OTHER);
    let location = oversized_resume
        .headers()
        .get(header::LOCATION)
        .and_then(|value| value.to_str().ok())
        .expect("redirect location");
    assert!(location.starts_with("https://chatgpt.com/connector/oauth/callback?"));
    assert!(location.contains("error=invalid_request"));
}

#[tokio::test]
async fn mcp_requires_a_bearer_token_and_serves_the_tool_catalog() {
    let request = Request::post("/mcp")
        .header("content-type", "application/json")
        .header(header::ACCEPT, "application/json, text/event-stream")
        .body(Body::from(
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#,
        ))
        .expect("request");
    let denied = mcp_app().oneshot(request).await.expect("response");
    assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);
    assert!(denied.headers().contains_key(header::WWW_AUTHENTICATE));

    let request = Request::post("/mcp")
        .header("content-type", "application/json")
        .header(header::ACCEPT, "application/json, text/event-stream")
        .header(header::AUTHORIZATION, "Bearer valid-token")
        .body(Body::from(
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#,
        ))
        .expect("request");
    let allowed = mcp_app().oneshot(request).await.expect("response");
    assert_eq!(allowed.status(), StatusCode::OK);
    let body = to_bytes(allowed.into_body(), usize::MAX)
        .await
        .expect("tool catalog body");
    let catalog: serde_json::Value = serde_json::from_slice(&body).expect("tool catalog");
    let tools = catalog["result"]["tools"].as_array().expect("tools array");
    assert!(tools.iter().any(|tool| tool["name"] == "get_note_profile"));
    assert!(
        tools
            .iter()
            .all(|tool| tool["inputSchema"]["additionalProperties"] == false)
    );

    let profile = mcp_app()
            .oneshot(
                Request::post("/mcp")
                    .header("content-type", "application/json")
                    .header(header::ACCEPT, "application/json, text/event-stream")
                    .header(header::AUTHORIZATION, "Bearer valid-token")
                    .body(Body::from(
                        r#"{"jsonrpc":"2.0","id":"profile","method":"tools/call","params":{"name":"get_note_profile"}}"#,
                    ))
                    .expect("request"),
            )
            .await
            .expect("profile response");
    let body = to_bytes(profile.into_body(), usize::MAX)
        .await
        .expect("profile body");
    let profile: serde_json::Value = serde_json::from_slice(&body).expect("profile JSON");
    assert_eq!(
        profile["result"]["structuredContent"]["adocweave_package_version"],
        "0.11.0"
    );
    assert_eq!(profile["result"]["structuredContent"]["profile_version"], 2);
    assert!(
        profile["result"]["structuredContent"]["examples"]
            .as_array()
            .is_some_and(|examples| !examples.is_empty())
    );

    let request = Request::post("/mcp")
        .header("content-type", "application/json")
        .header(header::ACCEPT, "APPLICATION/JSON, text/event-stream")
        .header(header::AUTHORIZATION, "Bearer valid-token")
        .body(Body::from(
            r#"{"jsonrpc":"2.0","id":"list","method":"tools/call","params":{"name":"list_notes"}}"#,
        ))
        .expect("request");
    let listed = mcp_app().oneshot(request).await.expect("response");
    assert_eq!(listed.status(), StatusCode::OK);
    let body = to_bytes(listed.into_body(), usize::MAX)
        .await
        .expect("response body");
    let listed: serde_json::Value = serde_json::from_slice(&body).expect("JSON-RPC response");
    assert!(listed["result"]["structuredContent"].is_object());
    assert!(listed["result"]["structuredContent"]["notes"].is_array());

    let request = Request::post("/mcp")
        .header("content-type", "Application/JSON")
        .header(header::ACCEPT, "application/json, text/event-stream")
        .header(header::AUTHORIZATION, "Bearer valid-token")
        .body(Body::from(r#"{"jsonrpc":"2.0","id":2,"method":"ping"}"#))
        .expect("request");
    let ping = mcp_app().oneshot(request).await.expect("response");
    assert_eq!(ping.status(), StatusCode::OK);
}

#[tokio::test]
async fn mcp_bearer_scheme_is_case_insensitive_and_scope_failures_are_forbidden() {
    let lowercase_bearer = Request::post("/mcp")
        .header("content-type", "application/json")
        .header(header::ACCEPT, "application/json, text/event-stream")
        .header(header::AUTHORIZATION, "bearer valid-token")
        .body(Body::from(
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#,
        ))
        .expect("request");
    let allowed = mcp_app().oneshot(lowercase_bearer).await.expect("response");
    assert_eq!(allowed.status(), StatusCode::OK);

    let insufficient_scope = Request::post("/mcp")
            .header("content-type", "application/json")
            .header(header::ACCEPT, "application/json, text/event-stream")
            .header(header::AUTHORIZATION, "Bearer read-token")
            .body(Body::from(
                r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"create_note","arguments":{"source":"= Title\n\nBody"}}}"#,
            ))
            .expect("request");
    let denied = mcp_app()
        .oneshot(insufficient_scope)
        .await
        .expect("response");
    assert_eq!(denied.status(), StatusCode::FORBIDDEN);
    assert!(
        denied
            .headers()
            .get(header::WWW_AUTHENTICATE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| {
                value.contains("error=\"insufficient_scope\"")
                    && value.contains("scope=\"notes:write\"")
            })
    );

    let write_only_profile = Request::post("/mcp")
            .header("content-type", "application/json")
            .header(header::ACCEPT, "application/json, text/event-stream")
            .header(header::AUTHORIZATION, "Bearer write-token")
            .body(Body::from(
                r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"get_note_profile","arguments":{}}}"#,
            ))
            .expect("request");
    let allowed = mcp_app()
        .oneshot(write_only_profile)
        .await
        .expect("response");
    assert_eq!(allowed.status(), StatusCode::OK);
}

#[tokio::test]
async fn mcp_rejects_invalid_json_rpc_envelopes_and_reports_tool_errors_as_results() {
    for (body, expected_id) in [
        (r#"{"id":1,"method":"tools/list"}"#, serde_json::json!(1)),
        (
            r#"{"jsonrpc":"2.0","id":true,"method":"tools/list"}"#,
            serde_json::Value::Null,
        ),
        (
            r#"{"jsonrpc":"2.0","id":null,"method":"tools/list"}"#,
            serde_json::Value::Null,
        ),
        (
            r#"{"jsonrpc":"2.0","id":1.5,"method":"tools/list"}"#,
            serde_json::Value::Null,
        ),
        (
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/list","params":[]}"#,
            serde_json::json!(1),
        ),
    ] {
        let response = mcp_app()
            .oneshot(
                Request::post("/mcp")
                    .header("content-type", "application/json")
                    .header(header::ACCEPT, "application/json, text/event-stream")
                    .header(header::AUTHORIZATION, "Bearer valid-token")
                    .body(Body::from(body))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        let response: serde_json::Value = serde_json::from_slice(&body).expect("JSON-RPC response");
        assert_eq!(response["error"]["code"], -32600);
        assert_eq!(response["id"], expected_id);
    }
    for (body, expected_code, expected_id) in [
        (r#"{"jsonrpc":"2.0","#, -32700, serde_json::Value::Null),
        (r#"{"jsonrpc":"2.0","id":1}"#, -32600, serde_json::json!(1)),
        (
            r#"[{"jsonrpc":"2.0","id":1,"method":"tools/list"}]"#,
            -32600,
            serde_json::Value::Null,
        ),
    ] {
        let response = mcp_app()
            .oneshot(
                Request::post("/mcp")
                    .header("content-type", "application/json")
                    .header(header::ACCEPT, "application/json, text/event-stream")
                    .body(Body::from(body))
                    .expect("request"),
            )
            .await
            .expect("response");
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        let response: serde_json::Value = serde_json::from_slice(&body).expect("JSON-RPC response");
        assert_eq!(response["error"]["code"], expected_code);
        assert_eq!(response["id"], expected_id);
    }

    let invalid_notification = mcp_app()
        .oneshot(
            Request::post("/mcp")
                .header("content-type", "application/json")
                .header(header::ACCEPT, "application/json, text/event-stream")
                .header(header::AUTHORIZATION, "Bearer valid-token")
                .body(Body::from(
                    r#"{"jsonrpc":"2.0","method":"tools/list","params":[]}"#,
                ))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(invalid_notification.status(), StatusCode::BAD_REQUEST);
    assert!(
        to_bytes(invalid_notification.into_body(), usize::MAX)
            .await
            .expect("response body")
            .is_empty()
    );

    let invalid_arguments = mcp_app()
            .oneshot(
                Request::post("/mcp")
                    .header("content-type", "application/json")
                    .header(header::ACCEPT, "application/json, text/event-stream")
                    .header(header::AUTHORIZATION, "Bearer valid-token")
                    .body(Body::from(
                        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"list_notes","arguments":[]}}"#,
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");
    let body = to_bytes(invalid_arguments.into_body(), usize::MAX)
        .await
        .expect("response body");
    let response: serde_json::Value = serde_json::from_slice(&body).expect("JSON-RPC response");
    assert_eq!(response["error"]["code"], -32602);

    let response = mcp_app()
            .oneshot(
                Request::post("/mcp")
                    .header("content-type", "application/json")
                    .header(header::ACCEPT, "application/json, text/event-stream")
                    .header(header::AUTHORIZATION, "Bearer valid-token")
                    .body(Body::from(
                        r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"create_note","arguments":{"source":"= Title\n\nBody"}}}"#,
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body");
    let response: serde_json::Value = serde_json::from_slice(&body).expect("JSON-RPC response");
    assert_eq!(response["result"]["isError"], true);
    assert_eq!(
        response["result"]["structuredContent"]["code"],
        "unavailable"
    );
    assert!(response.get("error").is_none());

    let validation = mcp_app()
            .oneshot(
                Request::post("/mcp")
                    .header("content-type", "application/json")
                    .header(header::ACCEPT, "application/json, text/event-stream")
                    .header(header::AUTHORIZATION, "Bearer valid-token")
                    .body(Body::from(
                        r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"create_note","arguments":{"source":"invalid"}}}"#,
                    ))
                    .expect("request"),
            )
            .await
            .expect("validation response");
    let body = to_bytes(validation.into_body(), usize::MAX)
        .await
        .expect("validation body");
    let validation: serde_json::Value = serde_json::from_slice(&body).expect("validation JSON");
    assert!(validation.get("error").is_none());
    assert_eq!(validation["result"]["isError"], true);
    assert_eq!(
        validation["result"]["structuredContent"]["code"],
        "validation_failed"
    );
    assert_eq!(
        validation["result"]["structuredContent"]["diagnostics"][0]["target"]["field"],
        "source"
    );
    assert!(
        validation["result"]["structuredContent"]["diagnostics"][0]
            .get("span")
            .is_none()
    );
    assert_eq!(
        validation["result"]["structuredContent"]["diagnostics"][1]["span"]["unit"],
        "utf8_byte"
    );
    let text: serde_json::Value =
        serde_json::from_str(validation["result"]["content"][0]["text"].as_str().unwrap())
            .expect("serialized structured error");
    assert_eq!(text, validation["result"]["structuredContent"]);
}

#[tokio::test]
async fn mcp_negotiates_initialization_and_validates_the_protocol_header() {
    for (requested, expected) in [
        ("2025-11-25", "2025-11-25"),
        ("2025-03-26", "2025-03-26"),
        ("unsupported", "2025-11-25"),
    ] {
        let response = mcp_app()
                .oneshot(
                    Request::post("/mcp")
                        .header("content-type", "application/json")
                        .header(header::ACCEPT, "application/json, text/event-stream")
                        .header(header::AUTHORIZATION, "Bearer valid-token")
                        .body(Body::from(format!(
                            r#"{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{"protocolVersion":"{requested}","capabilities":{{}},"clientInfo":{{"name":"test","version":"1"}}}}}}"#
                        )))
                        .expect("request"),
                )
                .await
                .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        let response: serde_json::Value =
            serde_json::from_slice(&body).expect("initialize response");
        assert_eq!(response["result"]["protocolVersion"], expected);
    }

    let invalid_capabilities = mcp_app()
            .oneshot(
                Request::post("/mcp")
                    .header("content-type", "application/json")
                    .header(header::ACCEPT, "application/json, text/event-stream")
                    .header(header::AUTHORIZATION, "Bearer valid-token")
                    .body(Body::from(
                        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{"roots":false},"clientInfo":{"name":"test","version":"1"}}}"#,
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");
    let body = to_bytes(invalid_capabilities.into_body(), usize::MAX)
        .await
        .expect("response body");
    let response: serde_json::Value = serde_json::from_slice(&body).expect("initialize response");
    assert_eq!(response["error"]["code"], -32602);

    let invalid_version = mcp_app()
        .oneshot(
            Request::post("/mcp")
                .header("content-type", "application/json")
                .header(header::ACCEPT, "application/json, text/event-stream")
                .header(header::AUTHORIZATION, "Bearer valid-token")
                .header("mcp-protocol-version", "unsupported")
                .body(Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#,
                ))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(invalid_version.status(), StatusCode::BAD_REQUEST);

    let initialized = mcp_app()
        .oneshot(
            Request::post("/mcp")
                .header("content-type", "application/json")
                .header(header::ACCEPT, "application/json, text/event-stream")
                .header(header::AUTHORIZATION, "Bearer valid-token")
                .header("mcp-protocol-version", "2025-11-25")
                .body(Body::from(
                    r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
                ))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(initialized.status(), StatusCode::ACCEPTED);

    let unexpected_response = mcp_app()
        .oneshot(
            Request::post("/mcp")
                .header("content-type", "application/json")
                .header(header::ACCEPT, "application/json, text/event-stream")
                .body(Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"result":{"unexpected":true}}"#,
                ))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(unexpected_response.status(), StatusCode::BAD_REQUEST);

    let wrong_content_type = mcp_app()
        .oneshot(
            Request::post("/mcp")
                .header("content-type", "text/plain")
                .header(header::ACCEPT, "application/json, text/event-stream")
                .body(Body::from("{}"))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(
        wrong_content_type.status(),
        StatusCode::UNSUPPORTED_MEDIA_TYPE
    );
}

#[tokio::test]
async fn mcp_accepts_configured_browser_origins_and_rejects_others() {
    let request = Request::post("/mcp")
        .header("content-type", "application/json")
        .header(header::ACCEPT, "application/json, text/event-stream")
        .header(header::ORIGIN, "https://chatgpt.com")
        .body(Body::from(
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#,
        ))
        .expect("request");
    let allowed = mcp_app().oneshot(request).await.expect("response");
    assert_eq!(allowed.status(), StatusCode::UNAUTHORIZED);

    let request = Request::post("/mcp")
        .header("content-type", "application/json")
        .header(header::ACCEPT, "application/json, text/event-stream")
        .header(header::ORIGIN, "https://example.test")
        .body(Body::from(
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#,
        ))
        .expect("request");
    let same_origin = mcp_app().oneshot(request).await.expect("response");
    assert_eq!(same_origin.status(), StatusCode::UNAUTHORIZED);

    let request = Request::post("/mcp")
        .header("content-type", "application/json")
        .header(header::ACCEPT, "application/json, text/event-stream")
        .header(header::ORIGIN, "https://untrusted.example")
        .body(Body::from(
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#,
        ))
        .expect("request");
    let rejected = mcp_app().oneshot(request).await.expect("response");
    assert_eq!(rejected.status(), StatusCode::FORBIDDEN);

    let invalid_get = Request::get("/mcp")
        .header(header::ORIGIN, "https://untrusted.example")
        .body(Body::empty())
        .expect("request");
    let rejected = mcp_app().oneshot(invalid_get).await.expect("response");
    assert_eq!(rejected.status(), StatusCode::FORBIDDEN);

    let native_get = Request::get("/mcp").body(Body::empty()).expect("request");
    let unsupported = mcp_app().oneshot(native_get).await.expect("response");
    assert_eq!(unsupported.status(), StatusCode::METHOD_NOT_ALLOWED);
}

#[test]
fn mcp_registration_limiter_bounds_a_window() {
    let limiter = McpRegistrationRateLimiter::new(1, Duration::from_secs(60));
    let now = Instant::now();
    assert!(limiter.allow("https://chatgpt.com", now));
    assert!(!limiter.allow("https://chatgpt.com", now));
    assert!(limiter.allow("https://claude.ai", now));
    assert!(limiter.allow("https://chatgpt.com", now + Duration::from_secs(61)));
}

#[test]
fn browser_mutations_require_the_application_origin() {
    let state = ApiState::new(
        Arc::new(Notes),
        Arc::new(Sessions),
        Arc::new(Oidc),
        "/".into(),
        "https://example.test".into(),
    );
    let mut headers = HeaderMap::new();
    headers.insert(
        header::ORIGIN,
        "https://example.test".parse().expect("origin"),
    );
    headers.insert("sec-fetch-site", "same-origin".parse().expect("metadata"));
    assert!(validate_mutation_origin(&headers, &state).is_ok());

    headers.insert(
        header::ORIGIN,
        "https://chatgpt.com".parse().expect("origin"),
    );
    assert!(validate_mutation_origin(&headers, &state).is_err());

    headers.insert(
        header::ORIGIN,
        "https://example.test".parse().expect("origin"),
    );
    headers.insert("sec-fetch-site", "cross-site".parse().expect("metadata"));
    assert!(validate_mutation_origin(&headers, &state).is_err());
}
