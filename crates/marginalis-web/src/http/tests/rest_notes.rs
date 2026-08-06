use super::*;

#[tokio::test]
async fn rest_and_web_creation_routes_assign_their_server_side_sources() {
    let notes = Arc::new(UiNotes {
        notes: Vec::new(),
        render_fails: false,
        creation_sources: Mutex::new(Vec::new()),
        list_queries: Mutex::new(Vec::new()),
    });
    let app = TestApp::default()
        .authenticated()
        .notes(notes.clone())
        .router();
    for path in ["/api/v3/notes", "/api/v3/web/notes"] {
        let response = app
            .clone()
            .oneshot(
                Request::post(path)
                    .header("content-type", "application/json")
                    .header(header::ORIGIN, "https://example.test")
                    .header("sec-fetch-site", "same-origin")
                    .header(
                        header::COOKIE,
                        "marginalis_session=active-session; marginalis_csrf=session-csrf",
                    )
                    .header("x-csrf-token", "session-csrf")
                    .body(Body::from(r#"{"source":"= 題名\n\n本文"}"#))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }
    assert_eq!(
        *notes.creation_sources.lock().expect("creation source lock"),
        [NoteCreationSource::Rest, NoteCreationSource::Web]
    );
}

#[tokio::test]
async fn rest_list_forwards_creation_source_and_review_status_filters() {
    let notes = Arc::new(UiNotes {
        notes: Vec::new(),
        render_fails: false,
        creation_sources: Mutex::new(Vec::new()),
        list_queries: Mutex::new(Vec::new()),
    });
    let response = TestApp::default()
        .authenticated()
        .notes(notes.clone())
        .router()
        .oneshot(authenticated_request(
            "/api/v3/notes?created_via=mcp&review_status=reviewed",
        ))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        *notes.list_queries.lock().expect("list query lock"),
        [NoteListQuery {
            created_via: Some(NoteCreationSource::Mcp),
            review_status: Some(marginalis_domain::NoteReviewStatus::Reviewed),
        }]
    );
}

#[tokio::test]
async fn owner_reads_and_marks_the_current_note_revision_as_reviewed() {
    let note_id = "0197c9bc-0000-7000-8000-000000000002";
    let app = authenticated_app();
    let read = app
        .clone()
        .oneshot(authenticated_request(&format!(
            "/api/v3/notes/{note_id}/review"
        )))
        .await
        .expect("read response");
    assert_eq!(read.status(), StatusCode::OK);
    let read_body = to_bytes(read.into_body(), usize::MAX).await.expect("body");
    let read_json: serde_json::Value = serde_json::from_slice(&read_body).expect("JSON");
    assert_eq!(read_json["status"], "pending");
    assert_eq!(read_json["reviewer_subject"], serde_json::Value::Null);

    let marked = app
        .oneshot(
            Request::post(format!("/api/v3/notes/{note_id}/review"))
                .header(header::ORIGIN, "https://example.test")
                .header("sec-fetch-site", "same-origin")
                .header(
                    header::COOKIE,
                    "marginalis_session=active-session; marginalis_csrf=session-csrf",
                )
                .header("x-csrf-token", "session-csrf")
                .header(header::IF_MATCH, "\"rev-3\"")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("mark response");
    assert_eq!(marked.status(), StatusCode::OK);
    assert_eq!(marked.headers()[header::ETAG], "\"rev-4\"");
    let marked_body = to_bytes(marked.into_body(), usize::MAX)
        .await
        .expect("body");
    let marked_json: serde_json::Value = serde_json::from_slice(&marked_body).expect("JSON");
    assert_eq!(marked_json["status"], "reviewed");
    assert_eq!(marked_json["reviewed_revision"], 4);
    assert_eq!(marked_json["reviewer_subject"], "alice");
}

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
    assert_eq!(
        body[0]["scope_ceiling"],
        serde_json::json!([
            "notes:read",
            "notes:write",
            "notes:delete",
            "bibliography:read",
            "bibliography:write",
            "bibliography:delete"
        ]),
        "未設定時は同意履歴ではなく実効上限を返す"
    );
    assert_eq!(body[0]["scope_ceiling_revision"], 0);
    assert_eq!(body[0]["scope_ceiling_configured"], false);
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
    assert_eq!(body["scope_ceiling_configured"], true);
}

/// clientの上限を解除して未設定へ戻せる。
///
/// 解除できないと、狭めた上限を広げられず同意画面からも復旧できなくなる。
#[tokio::test]
async fn owner_can_clear_one_mcp_client_scope_ceiling() {
    async fn delete(revision: &str) -> Response {
        authenticated_mcp_app()
            .oneshot(
                Request::delete(format!(
                    "/api/v3/mcp-authorizations/consent-client/scope-ceiling?revision={revision}"
                ))
                .header(header::ORIGIN, "https://example.test")
                .header("sec-fetch-site", "same-origin")
                .header(
                    header::COOKIE,
                    "marginalis_session=active-session; marginalis_csrf=session-csrf",
                )
                .header("x-csrf-token", "session-csrf")
                .body(Body::empty())
                .expect("request"),
            )
            .await
            .expect("response")
    }

    let cleared = delete("1").await;
    assert_eq!(cleared.status(), StatusCode::OK);
    let body = to_bytes(cleared.into_body(), usize::MAX)
        .await
        .expect("response body");
    let body: serde_json::Value = serde_json::from_slice(&body).expect("JSON");
    assert_eq!(body["scope_ceiling_configured"], false);
    assert_eq!(body["scope_ceiling_revision"], 0);
    assert_eq!(
        body["scope_ceiling"],
        serde_json::json!([
            "notes:read",
            "notes:write",
            "notes:delete",
            "bibliography:read",
            "bibliography:write",
            "bibliography:delete"
        ]),
        "解除後は実効上限として対応する全scopeを返す"
    );

    assert_eq!(delete("2").await.status(), StatusCode::CONFLICT);
    assert_eq!(delete("0").await.status(), StatusCode::UNPROCESSABLE_ENTITY);
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
