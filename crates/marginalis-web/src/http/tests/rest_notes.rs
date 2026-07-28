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
                    r#"{"source":"= 題名\n:tags: 試験\n\n本文"}"#,
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
