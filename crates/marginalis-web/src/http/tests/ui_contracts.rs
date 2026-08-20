use super::*;

#[test]
fn math_macro_contract_limits_match_application_policy() {
    use marginalis_application::{
        MAX_MATH_MACRO_ARGUMENTS, MAX_MATH_MACRO_NAME_CHARACTERS,
        MAX_MATH_MACRO_REPLACEMENT_CHARACTERS, MAX_MATH_MACRO_TOTAL_BYTES, MAX_MATH_MACROS,
    };

    let openapi = marginalis_contract::openapi_document();
    let schemas = &openapi["components"]["schemas"];
    let macro_schema = &schemas["MathMacro"]["properties"];
    let list_schema = &schemas["MathMacroSettings"]["properties"]["macros"];
    assert_eq!(
        macro_schema["name"]["maxLength"],
        MAX_MATH_MACRO_NAME_CHARACTERS
    );
    assert_eq!(
        macro_schema["replacement"]["maxLength"],
        MAX_MATH_MACRO_REPLACEMENT_CHARACTERS
    );
    assert_eq!(
        macro_schema["argument_count"]["maximum"],
        MAX_MATH_MACRO_ARGUMENTS
    );
    assert_eq!(list_schema["maxItems"], MAX_MATH_MACROS);
    assert_eq!(
        list_schema["x-marginalis-max-name-replacement-bytes"],
        MAX_MATH_MACRO_TOTAL_BYTES
    );
}

#[test]
fn external_paths_preserve_the_configured_subpath() {
    assert_eq!(external_path("/", "/notes/123"), "/notes/123");
    assert!(valid_return_to("/notes/new?from=home", "/"));
    assert_eq!(
        external_path("/marginalis", "/notes/123"),
        "/marginalis/notes/123"
    );
    assert!(valid_return_to(
        "/marginalis/notes/new?from=home",
        "/marginalis"
    ));
    assert!(!valid_return_to("/notes/new?from=home", "/marginalis"));
    assert!(!valid_return_to("//notes.example.test/new", "/"));
    assert!(!valid_return_to("/\\evil.example", "/"));
    assert!(!valid_return_to("/\\\\evil.example", "/"));
    assert!(!valid_return_to("/\t/evil.example", "/"));
    assert!(!valid_return_to("/ /evil.example", "/"));
    assert!(!valid_return_to("/marginalis/../notes/new", "/marginalis"));
    assert!(!valid_return_to(
        "/notes/new?from=home\r\nLocation:%20https://evil.test",
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
async fn authenticated_home_preserves_list_query_in_application_config() {
    let response = ui_app(Vec::new(), false, "/marginalis")
        .oneshot(authenticated_request(
            "/?tag=research&updated_after=2026-07-01",
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body");
    let body = String::from_utf8(body.to_vec()).expect("HTML");
    assert!(
        body.contains("&quot;search&quot;:&quot;?tag=research&amp;updated_after=2026-07-01&quot;")
    );
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
    // ヘッダーの移動先は一覧要素で並べるため、本文だけを対象にする。
    let main = body
        .split_once("<main class=\"page-main")
        .and_then(|(_, rest)| rest.split_once('>'))
        .and_then(|(_, rest)| rest.split_once("</main>"))
        .map(|(content, _)| content)
        .expect("main content");
    assert!(main.contains("画面を読み込んでいます。"));
    assert!(!main.contains("<li>"));
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
async fn deleted_notes_page_has_a_distinct_react_route() {
    let response = ui_app(Vec::new(), false, "/marginalis")
        .oneshot(authenticated_request("/notes/deleted"))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body");
    let body = String::from_utf8(body.to_vec()).expect("HTML");
    assert!(body.contains("&quot;path&quot;:&quot;/notes/deleted&quot;"));
    assert!(!body.contains("削除済みノート"));
}

#[tokio::test]
async fn rendered_note_view_api_returns_related_note_metadata() {
    let source = ui_note("閲覧中");
    let mut notes = vec![source.clone()];
    for index in 2..=13 {
        let note = Note::restore(NoteRestore {
            note_id: NoteId::new(
                format!("0197c9bc-0000-7000-8000-{index:012x}")
                    .parse()
                    .expect("note ID"),
            ),
            owner: test_principal("https://id.example.test", "alice"),
            draft: NoteDraft {
                title: format!("関連ノート{index}"),
                source: "本文".into(),
                tags: vec!["z".into(), "a".into(), "m".into(), "<危険>".into()],
            },
            created_at: UnixMillis::new(1),
            updated_at: UnixMillis::new(index),
            revision: Revision::INITIAL,
            deleted_at: None,
            created_via: NoteCreationSource::Web,
            review: NoteReviewTracking::pending(),
        })
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
    assert!(payload["note"]["source"].is_string());
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

/// Viteが出力した配布物のすべてに、配信経路があることを確かめる。
///
/// 以前は配信する名前を経路へ書き並べていた。分割読み込みでchunkが増えたとき、経路の追加を
/// 忘れて404になり、moduleとして読み込めずに画面全体が空になった。名前を数えるのではなく、
/// 実際の出力を1件ずつ引いて確かめる。
#[tokio::test]
async fn every_bundled_asset_has_a_route() {
    let names = crate::http::assets::BUNDLE_FILES
        .iter()
        .map(|(name, _, _)| *name)
        .collect::<Vec<_>>();
    assert!(
        names.contains(&"editor.js"),
        "配布物の一覧が空か、想定と違います: {names:?}"
    );
    for name in names {
        let response = app()
            .oneshot(
                Request::get(format!("/assets/{name}"))
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("asset response");
        assert_eq!(response.status(), StatusCode::OK, "{name}を配信できません");
        // MIME typeが空だと、ブラウザーはmoduleとして読み込まない。
        let content_type = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_owned();
        assert!(
            !content_type.is_empty(),
            "{name}のMIME typeが空です: {content_type:?}"
        );
        if name.ends_with(".js") {
            assert_eq!(content_type, "text/javascript; charset=utf-8", "{name}");
        }
    }
}

/// 配布物にない名前は404にする。表に無いものを配信しない。
#[tokio::test]
async fn an_unknown_bundled_asset_is_not_found() {
    let response = app()
        .oneshot(
            Request::get("/assets/not-present.js")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("asset response");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
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

    let mathjax_javascript = app()
        .oneshot(
            Request::get("/assets/tex-svg.js")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("MathJax response");
    assert_eq!(mathjax_javascript.status(), StatusCode::OK);
    assert_eq!(
        mathjax_javascript.headers().get(header::CONTENT_TYPE),
        Some(
            &"text/javascript; charset=utf-8"
                .parse()
                .expect("content type")
        )
    );

    for extension in ["boldsymbol.js", "mathtools.js"] {
        let mathjax_extension = app()
            .oneshot(
                Request::get(format!("/assets/{extension}"))
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("MathJax extension response");
        assert_eq!(mathjax_extension.status(), StatusCode::OK);
        assert_eq!(
            mathjax_extension.headers().get(header::CONTENT_TYPE),
            Some(
                &"text/javascript; charset=utf-8"
                    .parse()
                    .expect("content type")
            )
        );
    }

    let mathjax_font = app()
        .oneshot(
            Request::get("/assets/mathjax-fonts/mathjax-newcm-font/svg/dynamic/double-struck.js")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("MathJax font response");
    assert_eq!(mathjax_font.status(), StatusCode::OK);
    assert_eq!(
        mathjax_font.headers().get(header::CONTENT_TYPE),
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

    let web_font = app()
        .oneshot(
            Request::get("/assets/fonts/noto-sans-jp-latin-wght-normal.woff2")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("Web font response");
    assert_eq!(web_font.status(), StatusCode::OK);
    assert_eq!(
        web_font.headers().get(header::CONTENT_TYPE),
        Some(&"font/woff2".parse().expect("content type"))
    );

    let missing_web_font = app()
        .oneshot(
            Request::get("/assets/fonts/not-present.woff2")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("missing Web font response");
    assert_eq!(missing_web_font.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn editor_pages_embed_subpath_configuration_without_note_content() {
    let create = ui_app(Vec::new(), false, "/marginalis")
        .oneshot(authenticated_request("/notes/new"))
        .await
        .expect("create page");
    assert_eq!(create.status(), StatusCode::OK);
    let content_security_policy = create
        .headers()
        .get(header::CONTENT_SECURITY_POLICY)
        .expect("Content-Security-Policy")
        .to_str()
        .expect("Content-Security-Policy value")
        .to_owned();
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
    let nonce = body
        .split_once("&quot;styleNonce&quot;:&quot;")
        .and_then(|(_, rest)| rest.split_once("&quot;"))
        .map(|(nonce, _)| nonce)
        .expect("application style nonce");
    assert_eq!(nonce.len(), 22);
    assert!(
        nonce
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    );
    assert!(content_security_policy.contains(&format!("style-src-elem 'self' 'nonce-{nonce}'")));
    assert!(content_security_policy.contains("style-src-attr 'unsafe-inline'"));
    assert!(content_security_policy.contains("font-src 'self' data:"));
    assert!(content_security_policy.contains("img-src 'self' data:"));

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

/// 図の要求は、認可を通したうえで点と線をそのまま写す。
#[tokio::test]
async fn the_note_graph_endpoint_returns_visible_notes_and_filters_by_word() {
    let app = ui_app(vec![ui_note("グラフビュー")], false, "/");
    let response = app
        .clone()
        .oneshot(authenticated_request("/api/v3/notes/graph"))
        .await
        .expect("graph response");
    assert_eq!(response.status(), StatusCode::OK);
    let graph: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("graph body"),
    )
    .expect("graph JSON");
    assert_eq!(graph["notes"].as_array().expect("notes").len(), 1);
    assert!(graph["notes"][0]["title"].is_string());
    // 本文は図へ渡さない。
    assert!(graph["notes"][0].get("source").is_none());
    assert!(graph["works"].as_array().expect("works").is_empty());

    let filtered = ui_app(vec![ui_note("グラフビュー")], false, "/")
        .oneshot(authenticated_request(
            "/api/v3/notes/graph?query=%E4%B8%80%E8%87%B4%E3%81%97%E3%81%AA%E3%81%84",
        ))
        .await
        .expect("filtered response");
    assert_eq!(filtered.status(), StatusCode::OK);
    let filtered: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(filtered.into_body(), usize::MAX)
            .await
            .expect("filtered body"),
    )
    .expect("filtered JSON");
    assert!(filtered["notes"].as_array().expect("notes").is_empty());
}

/// 範囲外の階層数と、note IDでない起点を受け付けない。
#[tokio::test]
async fn the_note_graph_endpoint_rejects_an_unusable_scope() {
    for path in [
        "/api/v3/notes/graph?origin=not-a-note-id",
        "/api/v3/notes/graph?origin=0197c9bc-0000-7000-8000-000000000001&depth=0",
        "/api/v3/notes/graph?origin=0197c9bc-0000-7000-8000-000000000001&depth=6",
    ] {
        let response = ui_app(vec![ui_note("グラフビュー")], false, "/")
            .oneshot(authenticated_request(path))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{path}");
    }
}

/// 認証していない要求は図を返さない。
#[tokio::test]
async fn the_note_graph_endpoint_requires_authentication() {
    let response = ui_app(vec![ui_note("グラフビュー")], false, "/")
        .oneshot(
            Request::get("/api/v3/notes/graph")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}
