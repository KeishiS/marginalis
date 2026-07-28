#[test]
fn external_paths_preserve_the_configured_subpath() {
    assert_eq!(external_path("/", "/notes/123"), "/notes/123");
    assert!(valid_return_to("/oauth/authorize?client_id=client", "/"));
    assert_eq!(
        external_path("/marginalis", "/notes/123"),
        "/marginalis/notes/123"
    );
    assert!(valid_return_to(
        "/marginalis/oauth/authorize?client_id=client",
        "/marginalis"
    ));
    assert!(!valid_return_to(
        "/oauth/authorize?client_id=client",
        "/marginalis"
    ));
    assert!(!valid_return_to("//oauth/authorize?client_id=client", "/"));
    assert!(!valid_return_to(
        "/oauth/authorize?client_id=client\r\nLocation:%20https://evil.test",
        "/"
    ));
}

#[tokio::test]
async fn health_is_public_but_notes_require_a_session() {
    let health = app()
        .oneshot(
            Request::get("/api/v3/health")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(health.status(), StatusCode::OK);
    let request_id = health
        .headers()
        .get(crate::REQUEST_ID_HEADER)
        .expect("request ID")
        .to_str()
        .expect("request ID value");
    assert_eq!(
        uuid::Uuid::parse_str(request_id)
            .expect("UUID request ID")
            .get_version_num(),
        7
    );
    assert_eq!(
        health.headers().get(header::CACHE_CONTROL),
        Some(&"no-store".parse().expect("header"))
    );
    let notes = app()
        .oneshot(
            Request::get("/api/v3/notes")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(notes.status(), StatusCode::UNAUTHORIZED);

    let ui = app()
        .oneshot(Request::get("/").body(Body::empty()).expect("request"))
        .await
        .expect("response");
    assert_eq!(ui.status(), StatusCode::TEMPORARY_REDIRECT);
    assert!(
        ui.headers()
            .get(header::LOCATION)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|location| location.starts_with("/auth/oidc/login?next="))
    );
}

#[tokio::test]
async fn authenticated_home_serves_only_the_react_application_shell() {
    let note = ui_note("安全 <script>alert(\"x\")</script> & '題名'");
    let response = ui_app(vec![note], false, "/")
        .oneshot(authenticated_request("/"))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(header::CONTENT_TYPE),
        Some(&"text/html; charset=utf-8".parse().expect("content type"))
    );
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body");
    let body = String::from_utf8(body.to_vec()).expect("HTML");
    assert!(body.contains("<html lang=\"ja\">"));
    assert!(body.contains("data-application-root"));
    assert!(body.contains("&quot;path&quot;:&quot;/&quot;"));
    assert!(!body.contains("安全"));
    assert!(!body.contains("<script>alert"));
}

#[tokio::test]
async fn authenticated_home_defers_the_empty_state_to_react() {
    let response = ui_app(Vec::new(), false, "/")
        .oneshot(authenticated_request("/"))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body");
    let body = String::from_utf8(body.to_vec()).expect("HTML");
    assert!(body.contains("画面を読み込んでいます。"));
    assert!(!body.contains("<li>"));
}

#[tokio::test]
async fn note_view_serves_the_react_shell_with_subpath_configuration() {
    let note = ui_note("<安全な題名>");
    let response = ui_app(vec![note], false, "/marginalis")
        .oneshot(authenticated_request(
            "/notes/0197c9bc-0000-7000-8000-000000000001",
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body");
    let body = String::from_utf8(body.to_vec()).expect("HTML");
    assert!(body.contains("<title>Marginalis</title>"));
    assert!(body.contains("href=\"/marginalis/assets/editor.css\""));
    assert!(body.contains("&quot;apiBase&quot;:&quot;/marginalis/api/v3&quot;"));
    assert!(
        body.contains("&quot;path&quot;:&quot;/notes/0197c9bc-0000-7000-8000-000000000001&quot;")
    );
    assert!(!body.contains("描画済み本文"));
}

#[tokio::test]
async fn rendered_note_view_api_returns_related_note_metadata() {
    let source = ui_note("閲覧中");
    let mut notes = vec![source.clone()];
    for index in 2..=13 {
        let note = Note::restore(
            NoteId::new(
                format!("0197c9bc-0000-7000-8000-{index:012x}")
                    .parse()
                    .expect("note ID"),
            ),
            Identity::new("https://id.example.test".into(), "alice".into()).expect("valid owner"),
            format!("関連ノート{index}"),
            "本文".into(),
            vec!["z".into(), "a".into(), "m".into(), "<危険>".into()],
            UnixMillis::new(1),
            UnixMillis::new(index),
            Revision::INITIAL,
            None,
        )
        .expect("consistent note");
        notes.push(note);
    }
    let response = ui_app(notes, false, "/marginalis")
        .oneshot(authenticated_request(&format!(
            "/api/v3/notes/{}/view",
            source.note_id()
        )))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body");
    let payload: serde_json::Value = serde_json::from_slice(&body).expect("view JSON");
    assert_eq!(payload["html"], "<article><p>描画済み本文</p></article>");
    assert_eq!(payload["access"], "edit");
    assert_eq!(
        payload["related"]["outgoing"]
            .as_array()
            .expect("outgoing notes")
            .len(),
        12
    );
    assert_eq!(payload["related"]["outgoing"][0]["title"], "関連ノート2");
    assert!(payload["note"]["body"].is_string());
}

#[tokio::test]
async fn rendered_note_view_api_maps_missing_and_render_failed_notes_to_stable_errors() {
    let missing = ui_app(Vec::new(), false, "/")
        .oneshot(authenticated_request(
            "/api/v3/notes/0197c9bc-0000-7000-8000-000000000001/view",
        ))
        .await
        .expect("missing response");
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);

    let render_failed = ui_app(vec![ui_note("題名")], true, "/")
        .oneshot(authenticated_request(
            "/api/v3/notes/0197c9bc-0000-7000-8000-000000000001/view",
        ))
        .await
        .expect("render response");
    assert_eq!(render_failed.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let body = to_bytes(render_failed.into_body(), usize::MAX)
        .await
        .expect("response body");
    let problem: serde_json::Value = serde_json::from_slice(&body).expect("problem JSON");
    assert_eq!(problem["code"], "render_failed");
}

#[tokio::test]
async fn frontend_assets_are_served_with_explicit_content_types() {
    let javascript = app()
        .oneshot(
            Request::get("/assets/editor.js")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("JavaScript response");
    assert_eq!(javascript.status(), StatusCode::OK);
    assert_eq!(
        javascript.headers().get(header::CONTENT_TYPE),
        Some(
            &"text/javascript; charset=utf-8"
                .parse()
                .expect("content type")
        )
    );
    assert_eq!(
        javascript.headers().get(header::CACHE_CONTROL),
        Some(&"no-store".parse().expect("cache control"))
    );

    let page_javascript = app()
        .oneshot(
            Request::get("/assets/page.js")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("page JavaScript response");
    assert_eq!(page_javascript.status(), StatusCode::OK);
    assert_eq!(
        page_javascript.headers().get(header::CONTENT_TYPE),
        Some(
            &"text/javascript; charset=utf-8"
                .parse()
                .expect("content type")
        )
    );

    let stylesheet = app()
        .oneshot(
            Request::get("/assets/editor.css")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("stylesheet response");
    assert_eq!(stylesheet.status(), StatusCode::OK);
    assert_eq!(
        stylesheet.headers().get(header::CONTENT_TYPE),
        Some(&"text/css; charset=utf-8".parse().expect("content type"))
    );
}

#[tokio::test]
async fn editor_pages_embed_subpath_configuration_without_note_content() {
    let create = ui_app(Vec::new(), false, "/marginalis")
        .oneshot(authenticated_request("/notes/new"))
        .await
        .expect("create page");
    assert_eq!(create.status(), StatusCode::OK);
    let body = to_bytes(create.into_body(), usize::MAX)
        .await
        .expect("create body");
    let body = String::from_utf8(body.to_vec()).expect("HTML");
    assert!(body.contains("data-application-root"));
    assert!(body.contains("&quot;apiBase&quot;:&quot;/marginalis/api/v3&quot;"));
    assert!(body.contains("&quot;basePath&quot;:&quot;/marginalis&quot;"));
    assert!(body.contains("&quot;path&quot;:&quot;/notes/new&quot;"));
    assert!(body.contains("src=\"/marginalis/assets/editor.js\""));
    assert!(body.contains("<noscript>"));

    let edit = ui_app(vec![ui_note("非公開の本文を埋め込まない")], false, "/")
        .oneshot(authenticated_request(
            "/notes/0197c9bc-0000-7000-8000-000000000001/edit",
        ))
        .await
        .expect("edit page");
    assert_eq!(edit.status(), StatusCode::OK);
    let body = to_bytes(edit.into_body(), usize::MAX)
        .await
        .expect("edit body");
    let body = String::from_utf8(body.to_vec()).expect("HTML");
    assert!(
        body.contains(
            "&quot;path&quot;:&quot;/notes/0197c9bc-0000-7000-8000-000000000001/edit&quot;"
        )
    );
    assert!(!body.contains("非公開の本文を埋め込まない"));
}

#[tokio::test]
async fn edit_page_defers_note_visibility_to_the_rest_api() {
    let response = ui_app(Vec::new(), false, "/")
        .oneshot(authenticated_request(
            "/notes/0197c9bc-0000-7000-8000-000000000001/edit",
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn openapi_is_served_from_the_embedded_specification() {
    let response = app()
        .oneshot(
            Request::get("/api/v3/openapi.json")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        OPENAPI_DOCUMENT,
        include_str!("../../../../../docs/openapi.json")
    );
}

#[tokio::test]
async fn every_rest_contract_has_a_registered_router_method() {
    for contract in marginalis_contract::REST_ROUTE_CONTRACTS {
        let request = Request::builder()
            .method(contract.method)
            .uri(contract.probe_path)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from("{}"))
            .expect("contract probe request");
        let response = app()
            .oneshot(request)
            .await
            .expect("contract probe response");
        assert_ne!(
            response.status(),
            StatusCode::NOT_FOUND,
            "{} {} is not registered",
            contract.method,
            contract.probe_path
        );
        assert_ne!(
            response.status(),
            StatusCode::METHOD_NOT_ALLOWED,
            "{} {} has no matching method",
            contract.method,
            contract.probe_path
        );
    }
}
