#[tokio::test]
async fn protected_resource_metadata_names_the_external_authorization_server() {
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
        serde_json::json!(["https://issuer.example.test/"])
    );
    assert_eq!(
        metadata["scopes_supported"],
        serde_json::json!(["notes:read", "notes:write", "notes:delete"])
    );
}

#[tokio::test]
async fn marginalis_does_not_expose_authorization_server_endpoints() {
    for path in [
        "/.well-known/oauth-authorization-server",
        "/oauth/authorize",
        "/oauth/register",
        "/oauth/token",
    ] {
        let response = mcp_app()
            .oneshot(Request::get(path).body(Body::empty()).expect("request"))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
    }
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
    assert!(denied.headers().contains_key(header::WWW_AUTHENTICATE));

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
    let tools = catalog["result"]["tools"].as_array().expect("tools array");
    assert!(tools.iter().any(|tool| tool["name"] == "get_note_profile"));
    assert!(
        tools
            .iter()
            .all(|tool| tool["inputSchema"]["additionalProperties"] == false)
    );
    assert!(tools.iter().all(|tool| {
        if matches!(
            tool["name"].as_str(),
            Some("create_note" | "update_note")
        ) {
            tool["outputSchema"]["type"] == "object"
                && tool["outputSchema"]["anyOf"]
                    .as_array()
                    .is_some_and(|variants| {
                variants.len() == 2
                    && tool["outputSchema"]["$defs"]["McpNoteRevisionOutput"]
                        ["additionalProperties"]
                        == false
                    && tool["outputSchema"]["$defs"]["ProblemResponse"]["additionalProperties"]
                        == false
                    })
        } else {
            tool["outputSchema"]["additionalProperties"] == false
        }
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
        "0.17.0"
    );
    assert_eq!(profile["result"]["structuredContent"]["profile_version"], 5);
    let bibliography = &profile["result"]["structuredContent"]["examples"][0];
    assert_eq!(bibliography["kind"], "bibliography");
    assert_eq!(
        bibliography["body"],
        "= 先行研究の整理\n:tags: 文献, 研究\n\nSmithらは、対象の手法が有効だと報告しています <<smith2024>>。\n\n[bibliography]\n== 参考文献\n\n* [[[smith2024]]] Smith, A. et al. _Example Paper_. Example Journal, 2024. https://doi.org/10.1234/replace-with-doi[DOI]"
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
        ["Use bibliographic metadata supplied by the user or an identified source. Never invent or infer authors, titles, publication years, DOIs, or other bibliographic metadata."]
    );
    let text: serde_json::Value =
        serde_json::from_str(profile["result"]["content"][0]["text"].as_str().expect("text"))
            .expect("serialized profile output");
    assert_eq!(text, profile["result"]["structuredContent"]);

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
            notes: vec![marginalis_contract::McpNoteSummary {
                note_id: "0197c9bc-0000-7000-8000-000000000002".into(),
                title: "同期ノート".into(),
                tags: vec!["同期".into(), "試験".into()],
                updated_at_ms: 2_000,
                revision: 3,
            }],
        }
    );
    let text: serde_json::Value =
        serde_json::from_str(listed["result"]["content"][0]["text"].as_str().expect("text"))
            .expect("serialized list output");
    assert_eq!(text, listed["result"]["structuredContent"]);

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
    let text: serde_json::Value =
        serde_json::from_str(fetched["result"]["content"][0]["text"].as_str().expect("text"))
            .expect("serialized get output");
    assert_eq!(text, fetched["result"]["structuredContent"]);

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
        serde_json::json!({"code": "not_found", "message": "note was not found"})
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
        validation["result"]["structuredContent"]["diagnostics"][1]["span"]["unit"],
        "utf8_byte"
    );
    let text: serde_json::Value =
        serde_json::from_str(validation["result"]["content"][0]["text"].as_str().unwrap())
            .expect("serialized structured error");
    assert_eq!(text, validation["result"]["structuredContent"]);
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
                "name": "update_note",
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
        assert_eq!(structured["code"], "validation_failed");
        assert_eq!(structured["diagnostics"][0]["severity"], "warning");
        assert_eq!(
            structured["diagnostics"][0]["span"]["unit"],
            "utf8_byte"
        );
        assert_eq!(structured["diagnostics"][1]["severity"], "information");
        assert!(structured["diagnostics"][1].get("span").is_none());
        assert_eq!(structured["diagnostics"][2]["severity"], "hint");
        assert!(matches!(
            serde_json::from_value::<McpNoteMutationOutput>(structured.clone())
                .expect("mutation output contract"),
            McpNoteMutationOutput::Failure(_)
        ));
        let text: serde_json::Value = serde_json::from_str(
            response["result"]["content"][0]["text"]
                .as_str()
                .expect("text result"),
        )
        .expect("serialized warning result");
        assert_eq!(text, structured);
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
    let rejected = mcp_app()
        .oneshot(invalid_origin)
        .await
        .expect("response");
    assert_eq!(rejected.status(), StatusCode::FORBIDDEN);

    let native_get = Request::get("/mcp").body(Body::empty()).expect("request");
    let unsupported = mcp_app().oneshot(native_get).await.expect("response");
    assert_eq!(unsupported.status(), StatusCode::METHOD_NOT_ALLOWED);
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
