#[tokio::test]
async fn oidc_mcp_and_revocation_form_one_http_flow() {
    let server = TestServer::start().await;
    let client_id = register_mcp_client(&server.app).await;
    assert_cross_origin_authorization_post_starts_login(&server.app, &client_id).await;
    let user = login(
        &server,
        "user-subject",
        &["server-users"],
        "user-login-code",
    )
    .await;
    let issued_tokens = authorize_mcp(&server.app, &user, &client_id).await;
    let tokens = refresh_mcp(&server.app, &client_id, &issued_tokens.refresh).await;
    assert_ne!(tokens.access, issued_tokens.access);
    assert_ne!(tokens.refresh, issued_tokens.refresh);

    let mcp = McpTestClient::new(&server.app, "/mcp", &tokens.access);
    let initialized = mcp_json_response(
        mcp.request(
            1,
            "initialize",
            serde_json::json!({
                "protocolVersion": "2025-03-26",
                "capabilities": {},
                "clientInfo": {"name": "marginalis-regression-client", "version": "1"},
            }),
        )
        .await,
    )
    .await;
    assert_eq!(initialized["jsonrpc"], "2.0");
    assert_eq!(initialized["id"], 1);
    assert_eq!(initialized["result"]["protocolVersion"], "2025-03-26");
    assert_eq!(
        mcp.notification("notifications/initialized").await.status(),
        StatusCode::ACCEPTED
    );

    let batch = mcp
        .raw(serde_json::json!([
            {"jsonrpc": "2.0", "id": 2, "method": "ping"}
        ]))
        .await;
    let batch = mcp_json_response(batch).await;
    assert_eq!(batch["error"]["code"], -32600);
    assert_eq!(batch["id"], serde_json::Value::Null);

    let unknown = mcp_json_response(
        mcp.request(3, "client/nonstandard", serde_json::json!({}))
            .await,
    )
    .await;
    assert_eq!(unknown["error"]["code"], -32601);
    assert_eq!(unknown["id"], 3);

    let profile = call_mcp(
        &server.app,
        &tokens.access,
        4,
        "get_note_profile",
        serde_json::json!({}),
    )
    .await;
    assert_eq!(profile.status(), StatusCode::OK);
    let profile = json_body(profile).await;
    assert_eq!(
        profile["result"]["structuredContent"]["adocweave_package_version"],
        "0.11.0"
    );
    assert_eq!(profile["result"]["structuredContent"]["profile_version"], 2);

    let invalid = call_mcp(
        &server.app,
        &tokens.access,
        5,
        "create_note",
        serde_json::json!({
            "title": "",
            "body": "[source,brainfuck]\n----\n+\n----",
            "tags": ["integration"],
        }),
    )
    .await;
    assert_eq!(invalid.status(), StatusCode::OK);
    let invalid = json_body(invalid).await;
    assert_eq!(invalid["result"]["isError"], true);
    assert_eq!(
        invalid["result"]["structuredContent"]["code"],
        "validation_failed"
    );
    assert_eq!(
        invalid["result"]["structuredContent"]["diagnostics"][0]["target"]["field"],
        "title"
    );
    assert_eq!(
        invalid["result"]["structuredContent"]["diagnostics"][1]["span"]["unit"],
        "utf8_byte"
    );

    let response = call_mcp(
        &server.app,
        &tokens.access,
        6,
        "create_note",
        serde_json::json!({
            "title": "Owned <integration> note",
            "body": "Created through the authenticated MCP endpoint.",
            "tags": ["integration"],
        }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let created = json_body(response).await;
    let note_id = created["result"]["structuredContent"]["note_id"]
        .as_str()
        .expect("created note ID");

    let other_user = login(
        &server,
        "other-user-subject",
        &["server-users"],
        "other-user-login-code",
    )
    .await;
    let other_tokens = authorize_mcp(&server.app, &other_user, &client_id).await;
    let response = call_mcp(
        &server.app,
        &other_tokens.access,
        7,
        "list_notes",
        serde_json::json!({}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        json_body(response).await["result"]["structuredContent"]["notes"],
        serde_json::json!([])
    );
    let response = call_mcp(
        &server.app,
        &other_tokens.access,
        8,
        "get_note",
        serde_json::json!({ "note_id": note_id }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let response = json_body(response).await;
    assert_eq!(response["result"]["isError"], true);
    assert_eq!(response["result"]["structuredContent"]["code"], "not_found");
    let response = call_mcp(
        &server.app,
        &other_tokens.access,
        9,
        "update_note",
        serde_json::json!({
            "note_id": note_id,
            "title": "Unauthorized update",
            "body": "Must not persist.",
            "tags": [],
            "expected_revision": 1,
        }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let response = json_body(response).await;
    assert_eq!(response["result"]["isError"], true);
    assert_eq!(response["result"]["structuredContent"]["code"], "not_found");
    let response = call_mcp(
        &server.app,
        &other_tokens.access,
        10,
        "delete_note",
        serde_json::json!({
            "note_id": note_id,
            "expected_revision": 1,
        }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let response = json_body(response).await;
    assert_eq!(response["result"]["isError"], true);
    assert_eq!(response["result"]["structuredContent"]["code"], "not_found");

    let response = send(
        &server.app,
        Request::get("/api/v3/notes")
            .header(header::COOKIE, other_user.cookies())
            .body(Body::empty())
            .expect("other user list request"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_body(response).await, serde_json::json!([]));
    let response = send(
        &server.app,
        Request::get(format!("/api/v3/notes/{note_id}"))
            .header(header::COOKIE, other_user.cookies())
            .body(Body::empty())
            .expect("other user read request"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let response = send(
        &server.app,
        Request::put(format!("/api/v3/notes/{note_id}"))
            .header(header::COOKIE, other_user.cookies())
            .header(header::ORIGIN, BROWSER_ORIGIN)
            .header("sec-fetch-site", "same-origin")
            .header("x-csrf-token", &other_user.csrf)
            .header(header::IF_MATCH, "\"rev-1\"")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                serde_json::json!({
                    "title": "Unauthorized REST update",
                    "body": "Must not persist.",
                    "tags": [],
                })
                .to_string(),
            ))
            .expect("other user update request"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let response = send(
        &server.app,
        Request::delete(format!("/api/v3/notes/{note_id}"))
            .header(header::COOKIE, other_user.cookies())
            .header(header::ORIGIN, BROWSER_ORIGIN)
            .header("sec-fetch-site", "same-origin")
            .header("x-csrf-token", &other_user.csrf)
            .header(header::IF_MATCH, "\"rev-1\"")
            .body(Body::empty())
            .expect("other user delete request"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let response = send(
        &server.app,
        Request::get("/api/v3/notes")
            .header(header::COOKIE, user.cookies())
            .body(Body::empty())
            .expect("REST list request"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let notes = json_body(response).await;
    assert_eq!(notes.as_array().expect("notes").len(), 1);

    let response = send(
        &server.app,
        Request::get("/api/v3/session")
            .header(header::COOKIE, user.cookies())
            .body(Body::empty())
            .expect("session request"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let session = json_body(response).await;
    assert_eq!(session["subject"], "user-subject");
    assert!(session.get("is_administrator").is_none());

    let response = send(
        &server.app,
        Request::get("/")
            .header(header::COOKIE, user.cookies())
            .body(Body::empty())
            .expect("UI list request"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = text_body(response).await;
    assert!(body.contains("data-application-root"));
    assert!(!body.contains("Owned"));

    let response = send(
        &server.app,
        Request::get(format!("/notes/{note_id}"))
            .header(header::COOKIE, user.cookies())
            .body(Body::empty())
            .expect("UI note request"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(text_body(response).await.contains("data-application-root"));

    let response = send(
        &server.app,
        Request::get(format!("/api/v3/notes/{note_id}/view"))
            .header(header::COOKIE, user.cookies())
            .body(Body::empty())
            .expect("rendered view request"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let view = json_body(response).await;
    assert_eq!(view["note"]["title"], "Owned <integration> note");
    assert!(
        view["html"]
            .as_str()
            .expect("rendered HTML")
            .contains("Created through the authenticated MCP endpoint.")
    );

    let response = send(
        &server.app,
        Request::get(format!("/api/v3/notes/{note_id}"))
            .header(header::COOKIE, user.cookies())
            .body(Body::empty())
            .expect("REST read request"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(header::ETAG),
        Some(&"\"rev-1\"".parse().expect("ETag"))
    );
    let note = json_body(response).await;
    let revision = note["revision"].as_i64().expect("revision");

    let response = send(
        &server.app,
        Request::put(format!("/api/v3/notes/{note_id}"))
            .header(header::COOKIE, user.cookies())
            .header(header::ORIGIN, BROWSER_ORIGIN)
            .header("sec-fetch-site", "same-origin")
            .header("x-csrf-token", &user.csrf)
            .header(header::IF_MATCH, format!("\"rev-{revision}\""))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                serde_json::json!({
                    "title": "Updated integration note",
                    "body": "Updated through REST.",
                    "tags": ["integration", "updated"],
                })
                .to_string(),
            ))
            .expect("REST update request"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(header::ETAG),
        Some(&format!("\"rev-{}\"", revision + 1).parse().expect("ETag"))
    );
    let updated = json_body(response).await;
    let revision = updated["revision"].as_i64().expect("updated revision");

    let response = send(
        &server.app,
        Request::get(format!("/api/v3/notes/{note_id}/source"))
            .header(header::COOKIE, user.cookies())
            .body(Body::empty())
            .expect("REST export request"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        text_body(response)
            .await
            .contains("= Updated integration note")
    );

    let response = send(
        &server.app,
        Request::delete(format!("/api/v3/notes/{note_id}"))
            .header(header::COOKIE, user.cookies())
            .header(header::ORIGIN, BROWSER_ORIGIN)
            .header("sec-fetch-site", "same-origin")
            .header("x-csrf-token", &user.csrf)
            .header(header::IF_MATCH, format!("\"rev-{revision}\""))
            .body(Body::empty())
            .expect("REST delete request"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let deleted = json_body(response).await;
    let revision = deleted["revision"].as_i64().expect("deleted revision");

    let response = send(
        &server.app,
        Request::post(format!("/api/v3/notes/{note_id}/restore"))
            .header(header::COOKIE, user.cookies())
            .header(header::ORIGIN, BROWSER_ORIGIN)
            .header("sec-fetch-site", "same-origin")
            .header("x-csrf-token", &user.csrf)
            .header(header::IF_MATCH, format!("\"rev-{revision}\""))
            .body(Body::empty())
            .expect("REST restore request"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        json_body(response).await["title"],
        "Updated integration note"
    );

    let member_with_an_unrelated_group = login(
        &server,
        "unrelated-group-subject",
        &["server-users", "unrelated-group"],
        "unrelated-group-login-code",
    )
    .await;
    let response = send(
        &server.app,
        Request::get("/api/v3/notes")
            .header(header::COOKIE, member_with_an_unrelated_group.cookies())
            .body(Body::empty())
            .expect("unrelated group member list request"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        json_body(response)
            .await
            .as_array()
            .expect("unrelated group member notes")
            .is_empty()
    );

    let response = send(
        &server.app,
        Request::delete(format!("/api/v3/mcp-authorizations/{client_id}"))
            .header(header::COOKIE, user.cookies())
            .header(header::ORIGIN, BROWSER_ORIGIN)
            .header("sec-fetch-site", "same-origin")
            .header("x-csrf-token", &user.csrf)
            .body(Body::empty())
            .expect("revocation request"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let response = call_mcp(
        &server.app,
        &tokens.access,
        5,
        "list_notes",
        serde_json::json!({}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let form = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("grant_type", "refresh_token")
        .append_pair("client_id", &client_id)
        .append_pair("resource", MCP_RESOURCE)
        .append_pair("refresh_token", &tokens.refresh)
        .finish();
    let response = send(
        &server.app,
        Request::post("/oauth/token")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from(form))
            .expect("revoked refresh request"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json_body(response).await["error"], "invalid_grant");
}

