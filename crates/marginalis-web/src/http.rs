//! v0.3.0専用のHTTP APIと早期閲覧UI。
//!
//! 旧公開API・ローカル管理者・ローカル`UserId`を参照しない。composition rootは
//! v0.3.0ではこのrouterだけを公開する。

mod assets;
mod auth;
mod error;
mod html;
mod mcp_transport;
mod notes;
mod oauth;
mod related_notes;
mod security;
mod state;
mod ui;

#[cfg(test)]
mod tests;

pub use state::{ApiState, McpEndpoint};

use super::{RequestId, assign_request_id};
use std::sync::Arc;

use axum::{
    Json, Router,
    extract::DefaultBodyLimit,
    http::{StatusCode, header},
    middleware,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use tower_http::trace::{DefaultOnResponse, TraceLayer};
use tracing::{Level, info_span};

use self::{
    assets::{editor_javascript, editor_stylesheet, page_javascript},
    auth::{begin_login, complete_login, logout},
    error::{HandlerResult, problem},
    mcp_transport::{mcp_post, mcp_unsupported_method},
    notes::{
        create_note, delete_note, export_note, list_notes, preview_note, read_note, read_note_acl,
        replace_note_acl, restore_note, session, update_note,
    },
    oauth::{
        mcp_authorize, mcp_authorize_consent, mcp_authorize_post, mcp_register_client,
        mcp_resource_metadata, mcp_server_metadata, mcp_token, revoke_mcp_authorization,
    },
    security::security_headers,
    ui::{access_note_page, create_note_page, edit_note_page, home, view_note},
};

pub const API_VERSION: &str = "v2";
pub const OPENAPI_DOCUMENT: &str = include_str!("../../../docs/openapi.json");

/// 配備先のサブパスを保ったノートURLを生成するHTTP adapter。
#[derive(Clone, Copy, Debug, Default)]
pub struct HttpNoteLinkResolver;

impl marginalis_application::NoteLinkResolver for HttpNoteLinkResolver {
    fn href(
        &self,
        context: &marginalis_application::NoteRenderContext,
        note_id: marginalis_domain::NoteId,
        anchor: Option<&str>,
    ) -> Option<String> {
        let prefix = &context.note_path_prefix;
        if !prefix.starts_with('/') || prefix.starts_with("//") || prefix.contains(['?', '#']) {
            return None;
        }
        let prefix = prefix.trim_end_matches('/');
        let path = format!("{prefix}/{note_id}");
        let mut url = url::Url::parse("https://marginalis.invalid")
            .ok()?
            .join(&path)
            .ok()?;
        if url.path() != path {
            return None;
        }
        url.set_fragment(anchor);
        Some(
            url.as_str()
                .strip_prefix("https://marginalis.invalid")?
                .to_owned(),
        )
    }
}

pub fn router(state: ApiState) -> Router {
    let mut router = Router::new()
        .route("/", get(home))
        .route("/notes/new", get(create_note_page))
        .route("/notes/{note_id}/edit", get(edit_note_page))
        .route("/notes/{note_id}/access", get(access_note_page))
        .route("/notes/{note_id}", get(view_note))
        .route("/assets/editor.js", get(editor_javascript))
        .route("/assets/editor.css", get(editor_stylesheet))
        .route("/assets/page.js", get(page_javascript))
        .route("/api/v2/openapi.json", get(openapi))
        .route("/auth/oidc/login", get(begin_login))
        .route("/auth/oidc/callback", get(complete_login))
        .route("/auth/logout", post(logout))
        .route(
            "/oauth/authorize",
            get(mcp_authorize)
                .post(mcp_authorize_post)
                .layer(DefaultBodyLimit::max(16 * 1024)),
        )
        .route(
            "/oauth/authorize/consent",
            post(mcp_authorize_consent).layer(DefaultBodyLimit::max(16 * 1024)),
        )
        .route(
            "/oauth/register",
            post(mcp_register_client).layer(DefaultBodyLimit::max(16 * 1024)),
        )
        .route(
            "/oauth/token",
            post(mcp_token).layer(DefaultBodyLimit::max(16 * 1024)),
        )
        .route(
            "/mcp",
            get(mcp_unsupported_method)
                .post(mcp_post)
                .delete(mcp_unsupported_method)
                .layer(DefaultBodyLimit::max(1024 * 1024)),
        )
        .route("/api/v2/health", get(health))
        .route("/api/v2/session", get(session))
        .route("/api/v2/notes", get(list_notes).post(create_note))
        .route("/api/v2/notes/preview", post(preview_note))
        .route(
            "/api/v2/notes/{note_id}",
            get(read_note).put(update_note).delete(delete_note),
        )
        .route("/api/v2/notes/{note_id}/restore", post(restore_note))
        .route(
            "/api/v2/notes/{note_id}/acl",
            get(read_note_acl).put(replace_note_acl),
        )
        .route("/api/v2/notes/{note_id}/source", get(export_note))
        .route(
            "/api/v2/mcp-authorizations/{client_id}",
            axum::routing::delete(revoke_mcp_authorization),
        );
    if let Some(endpoint) = state.mcp.as_ref() {
        let resource_metadata_path = url::Url::parse(&endpoint.metadata_uri)
            .expect("validated MCP resource metadata URL")
            .path()
            .to_owned();
        let server_metadata_path = url::Url::parse(&endpoint.authorization_server_metadata_uri)
            .expect("validated authorization server metadata URL")
            .path()
            .to_owned();
        router = router
            .route(&resource_metadata_path, get(mcp_resource_metadata))
            .route(&server_metadata_path, get(mcp_server_metadata));
    }
    router
        .with_state(state)
        // Axum applies the last layer first. Assign the ID before TraceLayer creates its span.
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(|request: &axum::http::Request<_>| {
                    let request_id = request
                        .extensions()
                        .get::<RequestId>()
                        .map(|id| id.0.as_str())
                        .unwrap_or("missing");
                    info_span!(
                        "http_request",
                        request_id,
                        method = %request.method(),
                        path = request.uri().path(),
                    )
                })
                .on_response(DefaultOnResponse::new().level(Level::INFO)),
        )
        .layer(middleware::from_fn(security_headers))
        .layer(middleware::from_fn(assign_request_id))
}

async fn openapi() -> Response {
    (
        [(header::CONTENT_TYPE, "application/json; charset=utf-8")],
        OPENAPI_DOCUMENT,
    )
        .into_response()
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({"status": "ok", "api_version": API_VERSION}))
}

fn mcp_endpoint(state: &ApiState) -> HandlerResult<&Arc<McpEndpoint>> {
    state
        .mcp
        .as_ref()
        .ok_or_else(|| problem(StatusCode::NOT_FOUND, "not_found", "MCP is not available"))
}
