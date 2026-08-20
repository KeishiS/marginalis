use super::*;
use async_trait::async_trait;
use axum::{
    body::{Body, to_bytes},
    http::{HeaderMap, HeaderValue, Request},
};
use marginalis_application::{NoteLinkResolver, NoteListQuery, NoteRenderContext};
use marginalis_contract::McpNoteMutationOutput;
use marginalis_domain::{
    Actor, AttachmentId, EntityId, Note, NoteCreationSource, NoteDraft, NoteId, NoteRestore,
    NoteReviewTracking, Revision, UnixMillis,
};
use std::sync::Mutex;
use tower::ServiceExt;

mod support;
use support::*;

#[test]
fn http_observability_classifies_response_outcomes() {
    assert_eq!(http_outcome(StatusCode::OK), "success");
    assert_eq!(http_outcome(StatusCode::FOUND), "success");
    assert_eq!(http_outcome(StatusCode::NOT_FOUND), "rejected");
    assert_eq!(http_outcome(StatusCode::SERVICE_UNAVAILABLE), "failure");
}

#[test]
fn rendered_resource_links_use_their_actual_ui_and_api_routes() {
    let note_id = NoteId::new(
        "0197c9bc-0000-7000-8000-000000000001"
            .parse::<EntityId>()
            .expect("UUIDv7"),
    );
    let attachment_id = "0197c9bc-0000-7000-8000-000000000002"
        .parse::<AttachmentId>()
        .expect("UUIDv7");
    let resolver = HttpNoteLinkResolver;
    let root = NoteRenderContext {
        base_path: "/".into(),
    };
    assert_eq!(
        resolver.href(&root, note_id, Some("section")),
        Some(format!("/notes/{note_id}#section"))
    );
    assert_eq!(
        resolver.attachment_href(&root, note_id, attachment_id),
        Some(format!(
            "/api/v3/notes/{note_id}/attachments/{attachment_id}/content"
        ))
    );

    let nested = NoteRenderContext {
        base_path: "/knowledge".into(),
    };
    assert_eq!(
        resolver.href(&nested, note_id, None),
        Some(format!("/knowledge/notes/{note_id}"))
    );
    assert_eq!(
        resolver.attachment_href(&nested, note_id, attachment_id),
        Some(format!(
            "/knowledge/api/v3/notes/{note_id}/attachments/{attachment_id}/content"
        ))
    );

    let invalid = NoteRenderContext {
        base_path: "//outside.example".into(),
    };
    assert_eq!(resolver.href(&invalid, note_id, None), None);
    assert_eq!(
        resolver.attachment_href(&invalid, note_id, attachment_id),
        None
    );
}

#[test]
fn observability_logs_safe_http_and_mcp_results() {
    let logs = global_captured_logs();
    logs.clear();
    let note_id = "0197c9bc-0000-7000-8000-000000000001";
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("test runtime");
    runtime.block_on(async {
        let response = app()
            .oneshot(
                Request::get(format!(
                    "/api/v3/notes/{note_id}?search=must-not-be-logged"
                ))
                .header(header::COOKIE, "__Host-marginalis_session=secret-cookie")
                .body(Body::empty())
                .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        let response = mcp_app()
            .oneshot(
                Request::post("/mcp")
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(header::ACCEPT, "application/json, text/event-stream")
                    .header(header::AUTHORIZATION, "Bearer secret-bearer")
                    .body(Body::from("not-json-secret"))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);

        let response = mcp_app()
            .oneshot(
                Request::post("/mcp")
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(header::ACCEPT, "application/json, text/event-stream")
                    .header(header::AUTHORIZATION, "Basic secret-basic")
                    .body(Body::from("malformed-auth-body"))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        let response = mcp_app()
            .oneshot(
                Request::post("/mcp")
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(header::ACCEPT, "application/json, text/event-stream")
                    .header(header::AUTHORIZATION, "Bearer read-token")
                    .body(Body::from(
                        r#"{"jsonrpc":"2.0","id":"list-id","method":"tools/call","params":{"name":"list_notes"}}"#,
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);

        let response = mcp_app()
            .oneshot(
                Request::post("/mcp")
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(header::ACCEPT, "application/json, text/event-stream")
                    .header(header::AUTHORIZATION, "Bearer write-token")
                    .body(Body::from(
                        r#"{"jsonrpc":"2.0","id":"unavailable-id","method":"tools/call","params":{"name":"create_note","arguments":{"source":"= Private title\n\nPrivate body"}}}"#,
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);

        let response = mcp_app()
            .oneshot(
                Request::post("/mcp")
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(header::ACCEPT, "application/json, text/event-stream")
                    .header(header::AUTHORIZATION, "Bearer write-token")
                    .body(Body::from(
                        r#"{"jsonrpc":"2.0","id":"private-id","method":"tools/call","params":{"name":"create_note","arguments":{"source":"private source"}}}"#,
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
    });

    let logs = logs.text();
    assert_log_line(
        &logs,
        &[
            "event=\"http.request.completed\"",
            "request_id=",
            "method=GET",
            "path=\"/api/v3/notes/{note_id}\"",
            "problem_code=\"authentication_required\"",
            "status=401",
            "outcome=\"rejected\"",
            "latency_ms=",
        ],
    );
    assert_log_line(
        &logs,
        &[
            "event=\"mcp.request.completed\"",
            "method=\"unknown\"",
            "outcome=\"rejected\"",
            "reason=\"parse-error\"",
        ],
    );
    assert_log_line(
        &logs,
        &[
            "event=\"mcp.authentication.failed\"",
            "reason=\"token-format\"",
        ],
    );
    assert_log_line(
        &logs,
        &[
            "event=\"mcp.tool.completed\"",
            "tool=\"list_notes\"",
            "outcome=\"success\"",
        ],
    );
    assert_log_line(
        &logs,
        &[
            "event=\"mcp.tool.completed\"",
            "tool=\"create_note\"",
            "outcome=\"failure\"",
            "reason=\"unavailable\"",
        ],
    );
    assert_log_line(
        &logs,
        &[
            "event=\"mcp.tool.completed\"",
            "tool=\"create_note\"",
            "outcome=\"rejected\"",
            "reason=\"validation\"",
        ],
    );
    for secret in [
        note_id,
        "must-not-be-logged",
        "secret-cookie",
        "secret-bearer",
        "not-json-secret",
        "private-id",
        "private source",
        "secret-basic",
        "malformed-auth-body",
        "list-id",
        "unavailable-id",
        "Private title",
        "Private body",
    ] {
        assert!(
            !logs.contains(secret),
            "logs contain secret fixture: {secret}"
        );
    }
}

use super::auth::{external_path, valid_return_to, validate_mutation_origin};

mod ui_contracts;

mod mcp_transport;

mod oauth;

mod rest_notes;

mod bibliography;
mod bibliography_import;
