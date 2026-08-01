use super::*;

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
