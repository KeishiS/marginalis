use super::*;

#[tokio::test]
async fn deleted_note_list_exposes_only_the_owner_projection() {
    let response = authenticated_app()
        .oneshot(authenticated_request("/api/v3/notes/deleted"))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body");
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&body).expect("JSON"),
        serde_json::json!([{
            "note_id": "0197c9bc-0000-7000-8000-000000000002",
            "title": "削除済みノート",
            "deleted_at_ms": 100,
            "purge_at_ms": 200,
            "revision": 2
        }])
    );
}

#[tokio::test]
async fn expired_restoration_returns_gone_with_a_stable_problem_code() {
    let response = authenticated_app()
        .oneshot(
            Request::post("/api/v3/notes/0197c9bc-0000-7000-8000-000000000001/restore")
                .header(header::ORIGIN, "https://example.test")
                .header("sec-fetch-site", "same-origin")
                .header(
                    header::COOKIE,
                    "marginalis_session=active-session; marginalis_csrf=session-csrf",
                )
                .header("x-csrf-token", "session-csrf")
                .header(header::IF_MATCH, "\"rev-99\"")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::GONE);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body");
    let problem: serde_json::Value = serde_json::from_slice(&body).expect("problem JSON");
    assert_eq!(problem["code"], "retention_expired");
}

#[tokio::test]
async fn owner_can_read_and_replace_math_macros() {
    let app = TestApp::default().authenticated().router();
    let read = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v3/math-macros")
                .header(
                    "cookie",
                    "marginalis_session=active-session; marginalis_csrf=session-csrf",
                )
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(read.status(), StatusCode::OK);
    let read_body = to_bytes(read.into_body(), usize::MAX).await.expect("body");
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&read_body).expect("JSON"),
        serde_json::json!({ "macros": [], "revision": 0 })
    );

    let replace = app
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/api/v3/math-macros")
                .header("content-type", "application/json")
                .header(
                    "cookie",
                    "marginalis_session=active-session; marginalis_csrf=session-csrf",
                )
                .header("x-csrf-token", "session-csrf")
                .header("origin", "https://example.test")
                .body(Body::from(
                    serde_json::json!({
                        "macros": [
                            {
                                "name": "argmax",
                                "replacement": "\\operatorname*{arg\\,max}",
                                "argument_count": 0
                            },
                            {
                                "name": "bm",
                                "replacement": "\\boldsymbol{#1}",
                                "argument_count": 1
                            }
                        ],
                        "revision": 0
                    })
                    .to_string(),
                ))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(replace.status(), StatusCode::OK);
    let replace_body = to_bytes(replace.into_body(), usize::MAX)
        .await
        .expect("body");
    let body: serde_json::Value = serde_json::from_slice(&replace_body).expect("JSON");
    assert_eq!(body["revision"], 1);
    assert_eq!(body["macros"][1]["name"], "bm");
}

#[tokio::test]
async fn owner_can_read_and_replace_their_mcp_scope_ceiling() {
    let app = authenticated_mcp_app();
    let read = app
        .clone()
        .oneshot(authenticated_request("/api/v3/mcp-scope-ceilings"))
        .await
        .expect("response");
    assert_eq!(read.status(), StatusCode::OK);
    let read_body = to_bytes(read.into_body(), usize::MAX).await.expect("body");
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&read_body).expect("JSON"),
        serde_json::json!({
            "supported_scopes": [
                "notes:read",
                "notes:write",
                "notes:delete",
                "bibliography:read",
                "bibliography:write",
                "bibliography:delete"
            ],
            "scopes": ["notes:read", "notes:write"],
            "revision": 2
        })
    );

    let replace = app
        .oneshot(
            Request::put("/api/v3/mcp-scope-ceilings")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::ORIGIN, "https://example.test")
                .header("sec-fetch-site", "same-origin")
                .header(
                    header::COOKIE,
                    "marginalis_session=active-session; marginalis_csrf=session-csrf",
                )
                .header("x-csrf-token", "session-csrf")
                .body(Body::from(
                    serde_json::json!({
                        "scopes": ["notes:read"],
                        "revision": 2
                    })
                    .to_string(),
                ))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(replace.status(), StatusCode::OK);
    let replace_body = to_bytes(replace.into_body(), usize::MAX)
        .await
        .expect("body");
    let body: serde_json::Value = serde_json::from_slice(&replace_body).expect("JSON");
    assert_eq!(body["scopes"], serde_json::json!(["notes:read"]));
    assert_eq!(body["revision"], 3);
}

#[tokio::test]
async fn owner_can_list_and_restrict_their_mcp_client_authorizations() {
    let app = authenticated_mcp_app();
    let read = app
        .clone()
        .oneshot(authenticated_request("/api/v3/mcp-authorizations"))
        .await
        .expect("response");
    assert_eq!(read.status(), StatusCode::OK);
    let body = to_bytes(read.into_body(), usize::MAX)
        .await
        .expect("response body");
    let body: serde_json::Value = serde_json::from_slice(&body).expect("JSON");
    assert_eq!(body[0]["client_id"], "consent-client");
    assert_eq!(body[0]["registration_method"], "dynamic");
    assert_eq!(
        body[0]["granted_scopes"],
        serde_json::json!(["notes:read", "notes:write"])
    );
    assert_eq!(body[0]["last_used_at_ms"], 2_000);
    assert_eq!(body[0]["active"], true);

    let replace = app
        .oneshot(
            Request::put("/api/v3/mcp-authorizations/consent-client/scope-ceiling")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::ORIGIN, "https://example.test")
                .header("sec-fetch-site", "same-origin")
                .header(
                    header::COOKIE,
                    "marginalis_session=active-session; marginalis_csrf=session-csrf",
                )
                .header("x-csrf-token", "session-csrf")
                .body(Body::from(
                    serde_json::json!({"scopes": ["notes:read"], "revision": 0}).to_string(),
                ))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(replace.status(), StatusCode::OK);
    let body = to_bytes(replace.into_body(), usize::MAX)
        .await
        .expect("response body");
    let body: serde_json::Value = serde_json::from_slice(&body).expect("JSON");
    assert_eq!(body["scope_ceiling"], serde_json::json!(["notes:read"]));
    assert_eq!(body["scope_ceiling_revision"], 1);
}

#[tokio::test]
async fn rest_validation_returns_the_shared_diagnostic_contract() {
    let response = authenticated_app()
        .oneshot(
            Request::post("/api/v3/notes")
                .header("content-type", "application/json")
                .header(header::ORIGIN, "https://example.test")
                .header("sec-fetch-site", "same-origin")
                .header(
                    header::COOKIE,
                    "marginalis_session=active-session; marginalis_csrf=session-csrf",
                )
                .header("x-csrf-token", "session-csrf")
                .body(Body::from(r#"{"source":"本文だけ"}"#))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body");
    let problem: serde_json::Value = serde_json::from_slice(&body).expect("problem JSON");
    assert_eq!(problem["code"], "validation_failed");
    assert_eq!(problem["diagnostics"][0]["code"], "invalid_title");
    assert_eq!(problem["diagnostics"][0]["target"]["field"], "source");
    assert!(problem["diagnostics"][0].get("span").is_none());
}

#[tokio::test]
async fn rest_mutations_require_one_strong_revision_etag() {
    let request = |if_match: Option<&str>| {
        let mut request = Request::put("/api/v3/notes/0197c9bc-0000-7000-8000-000000000001")
            .header("content-type", "application/json")
            .header(header::ORIGIN, "https://example.test")
            .header("sec-fetch-site", "same-origin")
            .header(
                header::COOKIE,
                "marginalis_session=active-session; marginalis_csrf=session-csrf",
            )
            .header("x-csrf-token", "session-csrf");
        if let Some(value) = if_match {
            request = request.header(header::IF_MATCH, value);
        }
        request
            .body(Body::from(r#"{"source":"= 題名\n\n本文"}"#))
            .expect("request")
    };
    let missing = authenticated_app()
        .oneshot(request(None))
        .await
        .expect("response");
    assert_eq!(missing.status(), StatusCode::PRECONDITION_REQUIRED);
    let invalid = authenticated_app()
        .oneshot(request(Some("rev-1")))
        .await
        .expect("response");
    assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn preview_uses_the_shared_validation_and_safe_rendering_contract() {
    let valid = authenticated_app()
        .oneshot(
            Request::post("/api/v3/notes/preview")
                .header("content-type", "application/json")
                .header(header::ORIGIN, "https://example.test")
                .header("sec-fetch-site", "same-origin")
                .header(
                    header::COOKIE,
                    "marginalis_session=active-session; marginalis_csrf=session-csrf",
                )
                .header("x-csrf-token", "session-csrf")
                .body(Body::from(
                    r#"{"source":"= 題名\n:marginalis-tags: 試験\n\n本文"}"#,
                ))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(valid.status(), StatusCode::OK);
    let body = to_bytes(valid.into_body(), usize::MAX)
        .await
        .expect("response body");
    let preview: serde_json::Value = serde_json::from_slice(&body).expect("preview JSON");
    assert_eq!(preview["html"], "<article><p>プレビュー</p></article>");
    assert_eq!(preview["diagnostics"], serde_json::json!([]));

    let update = authenticated_app()
        .oneshot(
            Request::post("/api/v3/notes/0197c9bc-0000-7000-8000-000000000001/preview")
                .header("content-type", "application/json")
                .header(header::ORIGIN, "https://example.test")
                .header("sec-fetch-site", "same-origin")
                .header(
                    header::COOKIE,
                    "marginalis_session=active-session; marginalis_csrf=session-csrf",
                )
                .header("x-csrf-token", "session-csrf")
                .body(Body::from(r#"{"source":"= 更新\n\n本文"}"#))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(update.status(), StatusCode::OK);

    let update_without_csrf = authenticated_app()
        .oneshot(
            Request::post("/api/v3/notes/0197c9bc-0000-7000-8000-000000000001/preview")
                .header("content-type", "application/json")
                .header(
                    header::COOKIE,
                    "marginalis_session=active-session; marginalis_csrf=session-csrf",
                )
                .body(Body::from(r#"{"source":"= 更新\n\n本文"}"#))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(update_without_csrf.status(), StatusCode::FORBIDDEN);

    let warning = authenticated_app()
        .oneshot(
            Request::post("/api/v3/notes/preview")
                .header("content-type", "application/json")
                .header(header::ORIGIN, "https://example.test")
                .header("sec-fetch-site", "same-origin")
                .header(
                    header::COOKIE,
                    "marginalis_session=active-session; marginalis_csrf=session-csrf",
                )
                .header("x-csrf-token", "session-csrf")
                .body(Body::from(
                    r#"{"source":"= 題名\n\n本文xref:note:0197c9bc-0000-7000-8000-000000000002[参照]"}"#,
                ))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(warning.status(), StatusCode::OK);
    let body = to_bytes(warning.into_body(), usize::MAX)
        .await
        .expect("response body");
    let preview: serde_json::Value = serde_json::from_slice(&body).expect("preview JSON");
    assert_eq!(preview["diagnostics"][0]["code"], "macro-boundary");
    assert_eq!(preview["diagnostics"][0]["severity"], "warning");
    assert_eq!(preview["diagnostics"][0]["target"]["field"], "source");
    assert_eq!(preview["diagnostics"][0]["span"]["unit"], "utf8_byte");

    let invalid = authenticated_app()
        .oneshot(
            Request::post("/api/v3/notes/preview")
                .header("content-type", "application/json")
                .header(header::ORIGIN, "https://example.test")
                .header("sec-fetch-site", "same-origin")
                .header(
                    header::COOKIE,
                    "marginalis_session=active-session; marginalis_csrf=session-csrf",
                )
                .header("x-csrf-token", "session-csrf")
                .body(Body::from(r#"{"source":"本文"}"#))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(invalid.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let body = to_bytes(invalid.into_body(), usize::MAX)
        .await
        .expect("response body");
    let problem: serde_json::Value = serde_json::from_slice(&body).expect("problem JSON");
    assert_eq!(problem["code"], "validation_failed");
    assert_eq!(problem["diagnostics"][0]["code"], "invalid_title");
}

/// 同じ失敗に対して、RESTとMCPが同じ`code`と`message`を返すことを確認する。
///
/// 以前はMCPだけが応答JSONを手で組み立てており、`not_found`の文言がRESTと異なっていた。
#[tokio::test]
async fn rest_and_mcp_report_the_same_failure() {
    let missing = "0197c9bc-0000-7000-8000-0000000000ff";

    let rest = authenticated_app()
        .oneshot(authenticated_request(&format!("/api/v3/notes/{missing}")))
        .await
        .expect("response");
    assert_eq!(rest.status(), StatusCode::NOT_FOUND);
    let body = to_bytes(rest.into_body(), usize::MAX)
        .await
        .expect("response body");
    let rest_problem: serde_json::Value = serde_json::from_slice(&body).expect("problem JSON");

    let request = Request::post("/mcp")
        .header("content-type", "application/json")
        .header(header::ACCEPT, "application/json, text/event-stream")
        .header(header::AUTHORIZATION, "Bearer valid-token")
        .body(Body::from(format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{{"name":"get_note","arguments":{{"note_id":"{missing}"}}}}}}"#
        )))
        .expect("request");
    let mcp = mcp_app().oneshot(request).await.expect("response");
    let body = to_bytes(mcp.into_body(), usize::MAX)
        .await
        .expect("response body");
    let mcp_response: serde_json::Value = serde_json::from_slice(&body).expect("JSON-RPC response");
    assert_eq!(mcp_response["result"]["isError"], true);

    assert_eq!(
        rest_problem, mcp_response["result"]["structuredContent"],
        "RESTとMCPは同じ失敗表現を返します"
    );
}
