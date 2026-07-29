#[tokio::test]
async fn oauth_scopes_limit_mcp_operations() {
    let server = TestServer::start().await;
    let client_id = register_mcp_client(&server.app).await;
    let user = login(
        &server,
        "scope-test-subject",
        &["server-users"],
        "scope-test-login-code",
    )
    .await;

    let read_tokens =
        authorize_mcp_with_scopes(&server.app, &user, &client_id, "notes:read").await;
    let response = call_mcp(
        &server.app,
        &read_tokens.access,
        1,
        "list_notes",
        serde_json::json!({}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let response = call_mcp(
        &server.app,
        &read_tokens.access,
        2,
        "create_note",
        serde_json::json!({ "source": "= Read token\n\nMust not create." }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert!(
        response
            .headers()
            .get(header::WWW_AUTHENTICATE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.contains("scope=\"notes:write\""))
    );

    let write_tokens =
        authorize_mcp_with_scopes(&server.app, &user, &client_id, "notes:write").await;
    let response = call_mcp(
        &server.app,
        &write_tokens.access,
        3,
        "create_note",
        serde_json::json!({ "source": "= Write token\n\nCreated with write scope." }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let created = json_body(response).await;
    let note_id = created["result"]["structuredContent"]["note_id"]
        .as_str()
        .expect("created note ID");
    let response = call_mcp(
        &server.app,
        &write_tokens.access,
        4,
        "list_notes",
        serde_json::json!({}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let response = call_mcp(
        &server.app,
        &write_tokens.access,
        5,
        "delete_note",
        serde_json::json!({ "note_id": note_id, "expected_revision": 1 }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    let delete_tokens =
        authorize_mcp_with_scopes(&server.app, &user, &client_id, "notes:delete").await;
    let response = call_mcp(
        &server.app,
        &delete_tokens.access,
        6,
        "delete_note",
        serde_json::json!({ "note_id": note_id, "expected_revision": 1 }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let deleted = json_body(response).await;
    assert_eq!(
        deleted["result"]["structuredContent"]["note_id"],
        note_id
    );
    assert_eq!(deleted["result"]["structuredContent"]["revision"], 2);
}
