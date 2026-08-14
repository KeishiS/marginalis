use super::*;

#[tokio::test]
async fn mcp_creation_assigns_the_mcp_source() {
    let notes = Arc::new(UiNotes {
        notes: Vec::new(),
        render_fails: false,
        creation_sources: Mutex::new(Vec::new()),
        list_queries: Mutex::new(Vec::new()),
    });
    let response = TestApp::default()
        .notes(notes.clone())
        .mcp(
            "https://example.test",
            vec!["https://chatgpt.com".into()],
            Arc::new(TestMcpAuthenticator),
        )
        .router()
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
    assert_eq!(
        *notes.creation_sources.lock().expect("creation source lock"),
        [NoteCreationSource::Mcp]
    );
}

#[tokio::test]
async fn mcp_list_forwards_creation_source_and_review_status_filters() {
    let notes = Arc::new(UiNotes {
        notes: Vec::new(),
        render_fails: false,
        creation_sources: Mutex::new(Vec::new()),
        list_queries: Mutex::new(Vec::new()),
    });
    let response = TestApp::default()
        .notes(notes.clone())
        .mcp(
            "https://example.test",
            vec!["https://chatgpt.com".into()],
            Arc::new(TestMcpAuthenticator),
        )
        .router()
        .oneshot(
            Request::post("/mcp")
                .header("content-type", "application/json")
                .header(header::ACCEPT, "application/json, text/event-stream")
                .header(header::AUTHORIZATION, "Bearer valid-token")
                .body(Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"list_notes","arguments":{"created_via":"rest","review_status":"pending"}}}"#,
                ))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        *notes.list_queries.lock().expect("list query lock"),
        [NoteListQuery {
            created_via: Some(NoteCreationSource::Rest),
            review_status: Some(marginalis_domain::NoteReviewStatus::Pending),
        }]
    );
}

#[tokio::test]
async fn protected_resource_metadata_names_the_internal_authorization_server() {
    let response = mcp_app()
        .oneshot(
            Request::get("/.well-known/oauth-protected-resource/mcp")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("metadata body");
    let metadata: serde_json::Value = serde_json::from_slice(&body).expect("metadata");
    assert_eq!(metadata["resource"], "https://example.test/mcp");
    assert_eq!(
        metadata["authorization_servers"],
        serde_json::json!(["https://example.test/"])
    );
    assert_eq!(
        metadata["scopes_supported"],
        serde_json::json!([
            "notes:read",
            "notes:write",
            "notes:delete",
            "notes:sync",
            "bibliography:read",
            "bibliography:write",
            "bibliography:delete"
        ])
    );
}

#[tokio::test]
async fn marginalis_exposes_internal_authorization_server_metadata() {
    let response = mcp_app()
        .oneshot(
            Request::get("/.well-known/oauth-authorization-server")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("metadata body");
    let metadata: serde_json::Value = serde_json::from_slice(&body).expect("metadata");
    assert_eq!(metadata["issuer"], "https://example.test/");
    assert_eq!(
        metadata["code_challenge_methods_supported"],
        serde_json::json!(["S256"])
    );
    assert_eq!(
        metadata["authorization_response_iss_parameter_supported"],
        true
    );
    assert_eq!(metadata["client_id_metadata_document_supported"], true);
}

#[tokio::test]
async fn protected_resource_metadata_uses_the_rfc_path_for_a_subpath() {
    let response = subpath_mcp_app()
        .oneshot(
            Request::get("/.well-known/oauth-protected-resource/marginalis/mcp")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
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
    assert!(
        denied
            .headers()
            .get(header::WWW_AUTHENTICATE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.contains("scope=\"notes:read\""))
    );

    let denied_before_media_negotiation = mcp_app()
        .oneshot(
            Request::post("/mcp")
                .header("content-type", "text/plain")
                .header(header::ACCEPT, "application/json")
                .body(Body::from("not JSON"))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(
        denied_before_media_negotiation.status(),
        StatusCode::UNAUTHORIZED
    );
    assert!(
        denied_before_media_negotiation
            .headers()
            .contains_key(header::WWW_AUTHENTICATE)
    );

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
    assert!(catalog["result"].get("resultType").is_none());
    assert!(catalog["result"].get("_meta").is_none());
    let tools = catalog["result"]["tools"].as_array().expect("tools array");
    assert!(tools.iter().any(|tool| tool["name"] == "get_note_profile"));
    let add_bibliography_item = tools
        .iter()
        .find(|tool| tool["name"] == "add_bibliography_item")
        .expect("add bibliography item tool");
    let description = add_bibliography_item["description"]
        .as_str()
        .expect("tool description");
    assert!(description.contains("exactly one bibliography item"));
    assert!(description.contains("bulk import"));
    assert!(!description.contains("Web UI"));
    assert!(!description.contains("REST API"));
    assert!(
        tools
            .iter()
            .all(|tool| tool["inputSchema"]["additionalProperties"] == false)
    );
    // すべてのtoolが、成功出力と共通の失敗出力の選択として出力schemaを公開する。
    assert!(tools.iter().all(|tool| {
        tool["outputSchema"]["type"] == "object"
            && tool["outputSchema"]["anyOf"]
                .as_array()
                .is_some_and(|variants| variants.len() == 2)
            && tool["outputSchema"]["$defs"]["Problem"]["additionalProperties"] == false
    }));

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
        "0.23.0"
    );
    assert_eq!(profile["result"]["structuredContent"]["profile_version"], 6);
    let bibliography = &profile["result"]["structuredContent"]["examples"][0];
    assert_eq!(bibliography["kind"], "bibliography");
    assert_eq!(
        bibliography["body"],
        "= 先行研究の整理\n:marginalis-tags: 文献, 研究\n\nSmithらは、対象の手法が有効だと報告しています <<smith2024>>。\n\n[bibliography]\n== 参考文献\n\n* [[[smith2024]]] Smith, A. et al. _Example Paper_. Example Journal, 2024. https://doi.org/10.1234/replace-with-doi[DOI]"
    );
    assert_eq!(
        profile["result"]["structuredContent"]["authoring_guidance"],
        serde_json::json!([
            "Use bibliographic metadata supplied by the user or an identified source. Never invent or infer authors, titles, publication years, DOIs, or other bibliographic metadata."
        ])
    );
    let profile_output: marginalis_contract::McpNoteProfileOutput =
        serde_json::from_value(profile["result"]["structuredContent"].clone())
            .expect("typed profile output");
    assert_eq!(
        profile_output.authoring_guidance,
        [
            "Use bibliographic metadata supplied by the user or an identified source. Never invent or infer authors, titles, publication years, DOIs, or other bibliographic metadata."
        ]
    );
    assert!(profile_output.warnings_reject_write);
    assert!(
        profile_output
            .advisory_rules
            .iter()
            .any(|rule| rule.code == "macro-boundary"
                && rule.severity == marginalis_contract::DiagnosticSeverityResponse::Warning)
    );
    let text = profile["result"]["content"][0]["text"]
        .as_str()
        .expect("text");
    assert!(text.contains("Note profile version 6"));
    assert!(text.contains("MCP note writes reject warnings: true"));
    assert!(text.contains("macro-boundary"));
    assert!(text.contains("Allowed source languages: rust"));
    assert!(text.contains("Examples:"));
    assert!(text.contains("bibliography — Complete document"));

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
    let listed_output: marginalis_contract::McpListNotesOutput =
        serde_json::from_value(listed["result"]["structuredContent"].clone())
            .expect("typed list output");
    assert_eq!(
        listed_output,
        marginalis_contract::McpListNotesOutput {
            notes: vec![marginalis_contract::NoteSummaryResponse {
                note_id: "0197c9bc-0000-7000-8000-000000000002".into(),
                title: "同期ノート".into(),
                tags: vec!["同期".into(), "試験".into()],
                updated_at_ms: 2_000,
                revision: 3,
                created_via: NoteCreationSource::Mcp,
                review_status: marginalis_domain::NoteReviewStatus::Pending,
                reviewed_revision: None,
                reviewed_at_ms: None,
            }],
        }
    );
    let text = listed["result"]["content"][0]["text"]
        .as_str()
        .expect("text");
    assert!(text.contains("1 visible note"));
    assert!(text.contains("同期ノート (revision 3)"));

    let synchronized = mcp_app()
        .oneshot(
            Request::post("/mcp")
                .header("content-type", "application/json")
                .header(header::ACCEPT, "application/json, text/event-stream")
                .header(header::AUTHORIZATION, "Bearer sync-token")
                .body(Body::from(
                    r#"{"jsonrpc":"2.0","id":"sync","method":"tools/call","params":{"name":"sync_notes","arguments":{"limit":100}}}"#,
                ))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(synchronized.status(), StatusCode::OK);
    let body = to_bytes(synchronized.into_body(), usize::MAX)
        .await
        .expect("response body");
    let synchronized: serde_json::Value = serde_json::from_slice(&body).expect("JSON-RPC response");
    let output: marginalis_contract::McpSyncNotesOutput =
        serde_json::from_value(synchronized["result"]["structuredContent"].clone())
            .expect("typed sync output");
    assert_eq!(output.phase, marginalis_contract::McpSyncPhase::Snapshot);
    assert_eq!(output.next_cursor, "next-sync-cursor");
    let text = synchronized["result"]["content"][0]["text"]
        .as_str()
        .expect("text");
    assert!(text.contains("Synchronization snapshot"));
    assert!(text.contains("next-sync-cursor"));

    let request = Request::post("/mcp")
        .header("content-type", "application/json")
        .header(header::ACCEPT, "application/json, text/event-stream")
        .header(header::AUTHORIZATION, "Bearer valid-token")
        .body(Body::from(
            r#"{"jsonrpc":"2.0","id":"get","method":"tools/call","params":{"name":"get_note","arguments":{"note_id":"0197c9bc-0000-7000-8000-000000000002"}}}"#,
        ))
        .expect("request");
    let fetched = mcp_app().oneshot(request).await.expect("response");
    assert_eq!(fetched.status(), StatusCode::OK);
    let body = to_bytes(fetched.into_body(), usize::MAX)
        .await
        .expect("response body");
    let fetched: serde_json::Value = serde_json::from_slice(&body).expect("JSON-RPC response");
    let fetched_output: marginalis_contract::McpGetNoteOutput =
        serde_json::from_value(fetched["result"]["structuredContent"].clone())
            .expect("typed get output");
    assert_eq!(fetched_output.updated_at_ms, 2_000);
    assert_eq!(fetched_output.tags, vec!["同期", "試験"]);
    assert_eq!(fetched_output.revision, 3);
    let text = fetched["result"]["content"][0]["text"]
        .as_str()
        .expect("text");
    assert!(text.contains("同期ノート (revision 3)"));
    assert!(text.contains("AsciiDoc source:"));

    let request = Request::post("/mcp")
        .header("content-type", "application/json")
        .header(header::ACCEPT, "application/json, text/event-stream")
        .header(header::AUTHORIZATION, "Bearer valid-token")
        .body(Body::from(
            r#"{"jsonrpc":"2.0","id":"hidden","method":"tools/call","params":{"name":"get_note","arguments":{"note_id":"0197c9bc-0000-7000-8000-000000000003"}}}"#,
        ))
        .expect("request");
    let hidden = mcp_app().oneshot(request).await.expect("response");
    let body = to_bytes(hidden.into_body(), usize::MAX)
        .await
        .expect("response body");
    let hidden: serde_json::Value = serde_json::from_slice(&body).expect("JSON-RPC response");
    assert_eq!(hidden["result"]["isError"], true);
    assert_eq!(
        hidden["result"]["structuredContent"],
        serde_json::json!({"code": "not_found", "message": "note is not available"})
    );

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
async fn mcp_rejects_malformed_authorization_headers_before_parsing_the_body() {
    let malformed_headers = [
        HeaderValue::from_static("Basic credentials"),
        HeaderValue::from_static("Bearer"),
        HeaderValue::from_static("Bearer first second"),
        HeaderValue::from_bytes(&[0xff]).expect("opaque non-UTF-8 header"),
    ];
    for authorization in malformed_headers {
        let response = mcp_app()
            .oneshot(
                Request::post("/mcp")
                    .header(header::CONTENT_TYPE, "text/plain")
                    .header(header::ACCEPT, "application/json")
                    .header(header::AUTHORIZATION, authorization)
                    .body(Body::from("not JSON"))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert!(
            response
                .headers()
                .get(header::WWW_AUTHENTICATE)
                .and_then(|value| value.to_str().ok())
                .is_some_and(|value| value.contains("error=\"invalid_token\""))
        );
    }
}

#[tokio::test]
async fn authorization_server_failure_returns_service_unavailable() {
    let response = mcp_app_with_authenticator(Arc::new(UnavailableMcpAuthenticator))
        .oneshot(
            Request::post("/mcp")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::ACCEPT, "application/json, text/event-stream")
                .header(header::AUTHORIZATION, "Bearer opaque-test-token")
                .body(Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#,
                ))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert!(!response.headers().contains_key(header::WWW_AUTHENTICATE));
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
                    && value.contains("scope=\"notes:read notes:write\"")
            })
    );

    let read_scope_for_sync = Request::post("/mcp")
        .header("content-type", "application/json")
        .header(header::ACCEPT, "application/json, text/event-stream")
        .header(header::AUTHORIZATION, "Bearer read-token")
        .body(Body::from(
            r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"sync_notes","arguments":{}}}"#,
        ))
        .expect("request");
    let denied = mcp_app()
        .oneshot(read_scope_for_sync)
        .await
        .expect("response");
    assert_eq!(denied.status(), StatusCode::FORBIDDEN);
    assert!(
        denied
            .headers()
            .get(header::WWW_AUTHENTICATE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.contains("scope=\"notes:read notes:sync\""))
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

    let notes_scope_for_bibliography = Request::post("/mcp")
        .header("content-type", "application/json")
        .header(header::ACCEPT, "application/json, text/event-stream")
        .header(header::AUTHORIZATION, "Bearer read-token")
        .body(Body::from(
            r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"search_bibliography","arguments":{"query":"graph"}}}"#,
        ))
        .expect("request");
    let denied = mcp_app()
        .oneshot(notes_scope_for_bibliography)
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
                    && value.contains("scope=\"notes:read bibliography:read\"")
            })
    );

    let bibliography_scope = Request::post("/mcp")
        .header("content-type", "application/json")
        .header(header::ACCEPT, "application/json, text/event-stream")
        .header(
            header::AUTHORIZATION,
            "Bearer bibliography-read-token",
        )
        .body(Body::from(
            r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"search_bibliography","arguments":{"query":"graph"}}}"#,
        ))
        .expect("request");
    let allowed = mcp_app()
        .oneshot(bibliography_scope)
        .await
        .expect("response");
    assert_eq!(allowed.status(), StatusCode::OK);
}

#[tokio::test]
async fn scope_challenges_accumulate_existing_and_required_scopes_for_every_protocol() {
    for protocol_version in ["2025-03-26", "2025-11-25", "2026-07-28"] {
        let modern = protocol_version == "2026-07-28";
        let mut params = serde_json::json!({
            "name": "delete_note",
            "arguments": {"id": "0197c9bc-0000-7000-8000-000000000001", "revision": 1}
        });
        if modern {
            params.as_object_mut().expect("params").insert(
                "_meta".into(),
                serde_json::json!({
                    "io.modelcontextprotocol/protocolVersion": protocol_version,
                    "io.modelcontextprotocol/clientCapabilities": {},
                    "io.modelcontextprotocol/clientInfo": {"name": "test", "version": "1"}
                }),
            );
        }
        let mut request = Request::post("/mcp")
            .header("content-type", "application/json")
            .header(header::ACCEPT, "application/json, text/event-stream")
            .header(header::AUTHORIZATION, "Bearer write-token")
            .header("mcp-protocol-version", protocol_version);
        if modern {
            request = request
                .header("mcp-method", "tools/call")
                .header("mcp-name", "delete_note");
        }
        let response = mcp_app()
            .oneshot(
                request
                    .body(Body::from(
                        serde_json::json!({
                            "jsonrpc": "2.0",
                            "id": 1,
                            "method": "tools/call",
                            "params": params
                        })
                        .to_string(),
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(
            response.status(),
            StatusCode::FORBIDDEN,
            "{protocol_version}"
        );
        assert!(
            response
                .headers()
                .get(header::WWW_AUTHENTICATE)
                .and_then(|value| value.to_str().ok())
                .is_some_and(|value| {
                    value.contains("error=\"insufficient_scope\"")
                        && value.contains("scope=\"notes:write notes:delete\"")
                }),
            "{protocol_version}"
        );
    }
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
                    .header(header::AUTHORIZATION, "Bearer valid-token")
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
        validation["result"]["structuredContent"]["diagnostics"][0]["position"],
        serde_json::json!({"line": 1, "column": 1})
    );
    assert_eq!(
        validation["result"]["structuredContent"]["diagnostics"][1]["span"]["unit"],
        "utf8_byte"
    );
    let text = validation["result"]["content"][0]["text"]
        .as_str()
        .expect("natural-language error");
    assert!(text.contains("note input is invalid (validation_failed)"));
    assert!(text.contains("error invalid_title"));
}

#[tokio::test]
async fn mcp_create_and_update_reject_warnings_with_typed_diagnostics() {
    let source = "= Warning\n\nThis isxref:note:0197c9bc-0000-7000-8000-000000000002[related].";
    let calls = [
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": "create-warning",
            "method": "tools/call",
            "params": {
                "name": "create_note",
                "arguments": {"source": source}
            }
        }),
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": "update-warning",
            "method": "tools/call",
            "params": {
                "name": "replace_note_source",
                "arguments": {
                    "note_id": "0197c9bc-0000-7000-8000-000000000002",
                    "source": source,
                    "expected_revision": 3
                }
            }
        }),
    ];

    for call in calls {
        let response = mcp_app()
            .oneshot(
                Request::post("/mcp")
                    .header("content-type", "application/json")
                    .header(header::ACCEPT, "application/json, text/event-stream")
                    .header(header::AUTHORIZATION, "Bearer write-token")
                    .body(Body::from(call.to_string()))
                    .expect("request"),
            )
            .await
            .expect("warning response");
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("warning response body");
        let response: serde_json::Value =
            serde_json::from_slice(&body).expect("warning response JSON");
        assert_eq!(response["result"]["isError"], true);
        let structured = response["result"]["structuredContent"].clone();
        assert_eq!(structured["code"], "advisories_rejected");
        assert_eq!(
            structured["message"],
            "1 warning must be resolved before saving; first: macro-boundary at line 3, column 8"
        );
        assert_eq!(structured["diagnostics"][0]["severity"], "warning");
        assert_eq!(structured["diagnostics"][0]["span"]["unit"], "utf8_byte");
        assert_eq!(
            structured["diagnostics"][0]["position"],
            serde_json::json!({"line": 3, "column": 8})
        );
        assert_eq!(structured["diagnostics"][1]["severity"], "information");
        assert!(structured["diagnostics"][1].get("span").is_none());
        assert_eq!(structured["diagnostics"][2]["severity"], "hint");
        assert!(matches!(
            serde_json::from_value::<McpNoteMutationOutput>(structured.clone())
                .expect("mutation output contract"),
            McpNoteMutationOutput::Failure(_)
        ));
        let text = response["result"]["content"][0]["text"]
            .as_str()
            .expect("text result");
        assert!(text.contains("1 warning must be resolved"));
        assert!(text.contains("warning macro-boundary at line 3, column 8"));
    }
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
        assert_eq!(
            response["result"]["instructions"],
            marginalis_contract::MCP_SERVER_INSTRUCTIONS
        );
        assert!(
            !response["result"]["instructions"]
                .as_str()
                .expect("server instructions")
                .contains("REST API")
        );
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
                .header(header::AUTHORIZATION, "Bearer valid-token")
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
                .header(header::AUTHORIZATION, "Bearer valid-token")
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
async fn mcp_2026_requests_are_stateless_self_describing_and_header_checked() {
    let metadata = serde_json::json!({
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientCapabilities": {},
        "io.modelcontextprotocol/clientInfo": {"name": "test", "version": "1"}
    });
    let discover = mcp_app()
        .oneshot(
            Request::post("/mcp")
                .header("content-type", "application/json")
                .header(header::ACCEPT, "application/json, text/event-stream")
                .header(header::AUTHORIZATION, "Bearer valid-token")
                .header("mcp-protocol-version", "2026-07-28")
                .header("mcp-method", "server/discover")
                .body(Body::from(
                    serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": "discover",
                        "method": "server/discover",
                        "params": {"_meta": metadata.clone()}
                    })
                    .to_string(),
                ))
                .expect("request"),
        )
        .await
        .expect("discover response");
    assert_eq!(discover.status(), StatusCode::OK);
    let body = to_bytes(discover.into_body(), usize::MAX)
        .await
        .expect("discover body");
    let discover: serde_json::Value = serde_json::from_slice(&body).expect("discover JSON");
    assert_eq!(discover["result"]["resultType"], "complete");
    assert_eq!(
        discover["result"]["supportedVersions"],
        serde_json::json!(["2026-07-28", "2025-11-25", "2025-03-26"])
    );
    assert_eq!(
        discover["result"]["capabilities"],
        serde_json::json!({"tools": {}})
    );
    assert_eq!(discover["result"]["cacheScope"], "private");
    assert_eq!(
        discover["result"]["instructions"],
        marginalis_contract::MCP_SERVER_INSTRUCTIONS
    );
    assert!(
        !discover["result"]["instructions"]
            .as_str()
            .expect("server instructions")
            .contains("REST API")
    );
    assert_eq!(
        discover["result"]["_meta"]["io.modelcontextprotocol/serverInfo"]["name"],
        "marginalis"
    );

    let list = mcp_app()
        .oneshot(
            Request::post("/mcp")
                .header("content-type", "application/json")
                .header(header::ACCEPT, "application/json, text/event-stream")
                .header(header::AUTHORIZATION, "Bearer valid-token")
                .header("mcp-protocol-version", "2026-07-28")
                .header("mcp-method", "tools/list")
                .body(Body::from(
                    serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": 1,
                        "method": "tools/list",
                        "params": {"_meta": metadata.clone()}
                    })
                    .to_string(),
                ))
                .expect("request"),
        )
        .await
        .expect("list response");
    assert_eq!(list.status(), StatusCode::OK);
    let body = to_bytes(list.into_body(), usize::MAX)
        .await
        .expect("list body");
    let list: serde_json::Value = serde_json::from_slice(&body).expect("list JSON");
    assert_eq!(list["result"]["resultType"], "complete");
    assert_eq!(list["result"]["cacheScope"], "private");
    assert_eq!(list["result"]["ttlMs"], 3_600_000);
    assert_eq!(list["result"]["tools"][0]["name"], "list_notes");
    let add_bibliography_item = list["result"]["tools"]
        .as_array()
        .expect("tools array")
        .iter()
        .find(|tool| tool["name"] == "add_bibliography_item")
        .expect("add bibliography item tool");
    assert_eq!(
        add_bibliography_item["description"],
        marginalis_contract::mcp_tool_contracts()
            .into_iter()
            .find(|tool| tool.name == marginalis_contract::McpToolName::AddBibliographyItem)
            .expect("add bibliography item contract")
            .description
    );

    let call = mcp_app()
        .oneshot(
            Request::post("/mcp")
                .header("content-type", "application/json")
                .header(header::ACCEPT, "application/json, text/event-stream")
                .header(header::AUTHORIZATION, "Bearer valid-token")
                .header("mcp-protocol-version", "2026-07-28")
                .header("mcp-method", "tools/call")
                .header("mcp-name", "list_notes")
                .body(Body::from(
                    serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": 2,
                        "method": "tools/call",
                        "params": {
                            "_meta": metadata.clone(),
                            "name": "list_notes",
                            "arguments": {}
                        }
                    })
                    .to_string(),
                ))
                .expect("request"),
        )
        .await
        .expect("call response");
    assert_eq!(call.status(), StatusCode::OK);
    let body = to_bytes(call.into_body(), usize::MAX)
        .await
        .expect("call body");
    let call: serde_json::Value = serde_json::from_slice(&body).expect("call JSON");
    assert_eq!(call["result"]["resultType"], "complete");
    assert_eq!(
        call["result"]["_meta"]["io.modelcontextprotocol/serverInfo"]["name"],
        "marginalis"
    );
    assert!(call["result"]["structuredContent"]["notes"].is_array());

    let mismatch = mcp_app()
        .oneshot(
            Request::post("/mcp")
                .header("content-type", "application/json")
                .header(header::ACCEPT, "application/json, text/event-stream")
                .header(header::AUTHORIZATION, "Bearer valid-token")
                .header("mcp-protocol-version", "2026-07-28")
                .header("mcp-method", "tools/call")
                .header("mcp-name", "get_note")
                .body(Body::from(
                    serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": 3,
                        "method": "tools/list",
                        "params": {"_meta": metadata}
                    })
                    .to_string(),
                ))
                .expect("request"),
        )
        .await
        .expect("mismatch response");
    assert_eq!(mismatch.status(), StatusCode::BAD_REQUEST);
    let body = to_bytes(mismatch.into_body(), usize::MAX)
        .await
        .expect("mismatch body");
    let mismatch: serde_json::Value = serde_json::from_slice(&body).expect("mismatch JSON");
    assert_eq!(mismatch["error"]["code"], -32020);

    let unsupported = mcp_app()
        .oneshot(
            Request::post("/mcp")
                .header("content-type", "application/json")
                .header(header::ACCEPT, "application/json, text/event-stream")
                .header(header::AUTHORIZATION, "Bearer valid-token")
                .header("mcp-protocol-version", "2099-01-01")
                .header("mcp-method", "tools/list")
                .body(Body::from(
                    r#"{"jsonrpc":"2.0","id":4,"method":"tools/list"}"#,
                ))
                .expect("request"),
        )
        .await
        .expect("unsupported response");
    assert_eq!(unsupported.status(), StatusCode::BAD_REQUEST);
    let body = to_bytes(unsupported.into_body(), usize::MAX)
        .await
        .expect("unsupported body");
    let unsupported: serde_json::Value = serde_json::from_slice(&body).expect("unsupported JSON");
    assert_eq!(unsupported["error"]["code"], -32022);
    assert_eq!(unsupported["error"]["data"]["requested"], "2099-01-01");

    let unknown = mcp_app()
        .oneshot(
            Request::post("/mcp")
                .header("content-type", "application/json")
                .header(header::ACCEPT, "application/json, text/event-stream")
                .header(header::AUTHORIZATION, "Bearer valid-token")
                .header("mcp-protocol-version", "2026-07-28")
                .header("mcp-method", "ping")
                .body(Body::from(
                    serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": 5,
                        "method": "ping",
                        "params": {"_meta": metadata}
                    })
                    .to_string(),
                ))
                .expect("request"),
        )
        .await
        .expect("unknown response");
    assert_eq!(unknown.status(), StatusCode::NOT_FOUND);
    let body = to_bytes(unknown.into_body(), usize::MAX)
        .await
        .expect("unknown body");
    let unknown: serde_json::Value = serde_json::from_slice(&body).expect("unknown JSON");
    assert_eq!(unknown["error"]["code"], -32601);
}

#[tokio::test]
async fn mcp_accepts_configured_browser_origins_and_rejects_others() {
    let request = Request::post("/mcp")
        .header("content-type", "application/json")
        .header(header::ACCEPT, "application/json, text/event-stream")
        .header(header::AUTHORIZATION, "Bearer valid-token")
        .header(header::ORIGIN, "https://chatgpt.com")
        .body(Body::from(
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#,
        ))
        .expect("request");
    let allowed = mcp_app().oneshot(request).await.expect("response");
    assert_eq!(allowed.status(), StatusCode::OK);

    let request = Request::post("/mcp")
        .header("content-type", "application/json")
        .header(header::ACCEPT, "application/json, text/event-stream")
        .header(header::AUTHORIZATION, "Bearer valid-token")
        .header(header::ORIGIN, "https://example.test")
        .body(Body::from(
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#,
        ))
        .expect("request");
    let same_origin = mcp_app().oneshot(request).await.expect("response");
    assert_eq!(same_origin.status(), StatusCode::OK);

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

    let invalid_origin = Request::get("/mcp")
        .header(
            header::ORIGIN,
            HeaderValue::from_bytes(&[0xff]).expect("opaque non-UTF-8 header"),
        )
        .body(Body::empty())
        .expect("request");
    let rejected = mcp_app().oneshot(invalid_origin).await.expect("response");
    assert_eq!(rejected.status(), StatusCode::FORBIDDEN);

    let native_get = Request::get("/mcp").body(Body::empty()).expect("request");
    let unsupported = mcp_app().oneshot(native_get).await.expect("response");
    assert_eq!(unsupported.status(), StatusCode::METHOD_NOT_ALLOWED);
}

#[test]
fn browser_mutations_require_the_application_origin() {
    let state = ApiState::new(
        Arc::new(Notes),
        Arc::new(MathMacros),
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
