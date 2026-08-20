//! v0.3.0専用のHTTP APIと早期閲覧UI。
//!
//! 旧公開API・ローカル管理者・ローカル`UserId`を参照しない。composition rootは
//! v0.3.0ではこのrouterだけを公開する。

mod assets;
mod auth;
mod bibliography;
mod bibliography_import;
mod error;
mod html;
mod math_macros;
mod mcp_scope_ceilings;
mod mcp_transport;
mod note_sync;
pub(crate) mod notes;
mod oauth;
mod resource_authorization;
mod security;
mod state;
mod ui;
mod webhooks;

#[cfg(test)]
mod tests;

pub use state::{ApiServices, ApiState, InvalidMcpEndpoint, McpEndpoint};
pub use ui::browser_smoke_shell;

use super::{RequestId, assign_request_id};
use std::{sync::Arc, time::Duration};

use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, MatchedPath},
    http::{StatusCode, header},
    middleware,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use marginalis_contract::ProblemCode;
use tower_http::trace::TraceLayer;
use tracing::Span;

use self::{
    assets::{bundle_asset, mathjax_font_javascript, web_font},
    auth::{begin_login, complete_login, logout},
    bibliography::{
        add_bibliography_item, delete_bibliography_item, search_bibliography,
        update_bibliography_item,
    },
    bibliography_import::{
        apply_bibliography_import, list_bibliography_import_sources, preview_bibliography_import,
    },
    error::{HandlerResult, problem},
    math_macros::{read_math_macros, replace_math_macros},
    mcp_scope_ceilings::{
        delete_client_mcp_scope_ceiling, list_mcp_authorizations, read_mcp_scope_ceiling,
        replace_client_mcp_scope_ceiling, replace_mcp_scope_ceiling,
    },
    mcp_transport::{mcp_post, mcp_unsupported_method},
    note_sync::sync_notes,
    notes::{
        compare_note_revisions, create_note, create_web_note, delete_note, delete_note_attachment,
        export_note, list_deleted_notes, list_note_attachments, list_note_revisions, list_notes,
        mark_note_reviewed, preview_new_note, preview_note_update, read_note, read_note_acl,
        read_note_attachment_content, read_note_graph, read_note_review, read_note_revision,
        read_note_view, replace_note_acl, restore_note, restore_note_revision, session,
        update_note, upload_note_attachment,
    },
    oauth::{
        mcp_authorize, mcp_authorize_consent, mcp_authorize_post, mcp_register_client,
        mcp_resource_metadata, mcp_revoke_token, mcp_server_metadata, mcp_token,
        revoke_mcp_authorization,
    },
    security::security_headers,
    ui::{
        access_note_page, bibliography_page, create_note_page, deleted_notes_page, edit_note_page,
        graph_page, home, math_macro_settings_page, mcp_access_settings_page, settings_page,
        view_note, webhook_settings_page,
    },
    webhooks::{
        create_webhook, delete_webhook, discard_webhook_delivery, list_webhooks,
        regenerate_webhook_secret, retry_webhook_delivery, verify_webhook,
    },
};

pub use marginalis_contract::API_VERSION;
pub const OPENAPI_DOCUMENT: &str = include_str!("../../../docs/openapi.json");
// 8 MiBのCSL-JSON配列に、取込元と最大1,000件分の選択を加えたJSON envelopeを受け取る。
const BIBLIOGRAPHY_IMPORT_REQUEST_BYTES: usize = 9 * 1024 * 1024;

/// 配備先のサブパスを保ったノートと添付画像のURLを生成するHTTP adapter。
#[derive(Clone, Copy, Debug, Default)]
pub struct HttpNoteLinkResolver;

impl marginalis_application::NoteLinkResolver for HttpNoteLinkResolver {
    fn href(
        &self,
        context: &marginalis_application::NoteRenderContext,
        note_id: marginalis_domain::NoteId,
        anchor: Option<&str>,
    ) -> Option<String> {
        resource_href(context, &format!("/notes/{note_id}"), anchor)
    }

    fn attachment_href(
        &self,
        context: &marginalis_application::NoteRenderContext,
        note_id: marginalis_domain::NoteId,
        attachment_id: marginalis_domain::AttachmentId,
    ) -> Option<String> {
        resource_href(
            context,
            &format!("/api/v3/notes/{note_id}/attachments/{attachment_id}/content"),
            None,
        )
    }
}

fn resource_href(
    context: &marginalis_application::NoteRenderContext,
    resource_path: &str,
    anchor: Option<&str>,
) -> Option<String> {
    let path = auth::external_path(&context.base_path, resource_path);
    if !path.starts_with('/') || path.starts_with("//") || path.contains(['?', '#']) {
        return None;
    }
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

pub fn router(state: ApiState) -> Router {
    let mut router = Router::new()
        .route("/", get(home))
        .route("/bibliography", get(bibliography_page))
        .route("/graph", get(graph_page))
        .route("/settings/math-macros", get(math_macro_settings_page))
        .route("/settings", get(settings_page))
        .route("/settings/mcp-access", get(mcp_access_settings_page))
        .route("/settings/webhooks", get(webhook_settings_page))
        .route("/notes/deleted", get(deleted_notes_page))
        .route("/notes/new", get(create_note_page))
        .route("/notes/{note_id}/edit", get(edit_note_page))
        .route("/notes/{note_id}/access", get(access_note_page))
        .route("/notes/{note_id}/history", get(view_note))
        .route("/notes/{note_id}", get(view_note))
        // 配布物の名前を書き並べず、ビルド時に作った表から引く。分割読み込みでchunkが増えても
        // 経路の追加を忘れて配信されない、という失敗が起きない。
        .route("/assets/{file_name}", get(bundle_asset))
        .route("/assets/fonts/{file_name}", get(web_font))
        .route(
            "/assets/mathjax-fonts/mathjax-newcm-font/svg/dynamic/{file_name}",
            get(mathjax_font_javascript),
        )
        .route("/api/v3/openapi.json", get(openapi))
        .route("/auth/oidc/login", get(begin_login))
        .route("/auth/oidc/callback", get(complete_login))
        .route("/auth/logout", post(logout))
        .route(
            "/mcp",
            get(mcp_unsupported_method)
                .post(mcp_post)
                .delete(mcp_unsupported_method)
                .layer(DefaultBodyLimit::max(1024 * 1024)),
        )
        .route("/api/v3/health", get(health))
        .route("/api/v3/session", get(session))
        .route(
            "/api/v3/math-macros",
            get(read_math_macros).put(replace_math_macros),
        )
        .route(
            "/api/v3/mcp-scope-ceilings",
            get(read_mcp_scope_ceiling).put(replace_mcp_scope_ceiling),
        )
        .route("/api/v3/mcp-authorizations", get(list_mcp_authorizations))
        .route(
            "/api/v3/mcp-authorizations/{client_id}/scope-ceiling",
            axum::routing::put(replace_client_mcp_scope_ceiling)
                .delete(delete_client_mcp_scope_ceiling),
        )
        .route("/api/v3/webhooks", get(list_webhooks).post(create_webhook))
        .route(
            "/api/v3/webhooks/{subscription_id}",
            axum::routing::delete(delete_webhook),
        )
        .route(
            "/api/v3/webhooks/{subscription_id}/verify",
            post(verify_webhook),
        )
        .route(
            "/api/v3/webhooks/{subscription_id}/secret",
            post(regenerate_webhook_secret),
        )
        .route(
            "/api/v3/webhooks/{subscription_id}/retry",
            post(retry_webhook_delivery),
        )
        .route(
            "/api/v3/webhooks/{subscription_id}/discard",
            post(discard_webhook_delivery),
        )
        .route(
            "/api/v3/bibliography",
            get(search_bibliography).post(add_bibliography_item),
        )
        .route(
            "/api/v3/bibliography/{item_id}",
            axum::routing::put(update_bibliography_item).delete(delete_bibliography_item),
        )
        .route(
            "/api/v3/bibliography/import-sources",
            get(list_bibliography_import_sources),
        )
        .route(
            "/api/v3/bibliography/import-previews",
            post(preview_bibliography_import)
                .layer(DefaultBodyLimit::max(BIBLIOGRAPHY_IMPORT_REQUEST_BYTES)),
        )
        .route(
            "/api/v3/bibliography/imports",
            post(apply_bibliography_import)
                .layer(DefaultBodyLimit::max(BIBLIOGRAPHY_IMPORT_REQUEST_BYTES)),
        )
        .route("/api/v3/notes", get(list_notes).post(create_note))
        .route("/api/v3/sync/notes", get(sync_notes))
        .route("/api/v3/web/notes", post(create_web_note))
        .route("/api/v3/notes/deleted", get(list_deleted_notes))
        .route("/api/v3/notes/preview", post(preview_new_note))
        .route("/api/v3/notes/{note_id}/preview", post(preview_note_update))
        .route(
            "/api/v3/notes/{note_id}",
            get(read_note).put(update_note).delete(delete_note),
        )
        .route("/api/v3/notes/{note_id}/view", get(read_note_view))
        .route(
            "/api/v3/notes/{note_id}/attachments",
            get(list_note_attachments)
                .post(upload_note_attachment)
                .layer(DefaultBodyLimit::max(
                    marginalis_domain::ATTACHMENT_POLICY.max_bytes,
                )),
        )
        .route(
            "/api/v3/notes/{note_id}/attachments/{attachment_id}",
            axum::routing::delete(delete_note_attachment),
        )
        .route(
            "/api/v3/notes/{note_id}/attachments/{attachment_id}/content",
            get(read_note_attachment_content),
        )
        .route("/api/v3/notes/{note_id}/history", get(list_note_revisions))
        .route(
            "/api/v3/notes/{note_id}/history/{revision}",
            get(read_note_revision),
        )
        .route(
            "/api/v3/notes/{note_id}/history/{revision}/restore",
            post(restore_note_revision),
        )
        .route(
            "/api/v3/notes/{note_id}/history-diff",
            get(compare_note_revisions),
        )
        .route("/api/v3/notes/{note_id}/restore", post(restore_note))
        .route(
            "/api/v3/notes/{note_id}/acl",
            get(read_note_acl).put(replace_note_acl),
        )
        .route(
            "/api/v3/notes/{note_id}/review",
            get(read_note_review).post(mark_note_reviewed),
        )
        .route("/api/v3/notes/{note_id}/source", get(export_note))
        .route("/api/v3/notes/graph", get(read_note_graph))
        // 公開RESTの形をMCPの設定で変えない。MCPが無効な構成では、経路を隠さず利用不可を返す。
        .route(
            "/api/v3/mcp-authorizations/{client_id}",
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
            .route(&server_metadata_path, get(mcp_server_metadata))
            .route(
                "/oauth/authorize",
                get(mcp_authorize).post(mcp_authorize_post),
            )
            .route("/oauth/authorize/consent", post(mcp_authorize_consent))
            .route(
                "/oauth/register",
                post(mcp_register_client).layer(DefaultBodyLimit::max(16 * 1024)),
            )
            .route(
                "/oauth/token",
                post(mcp_token).layer(DefaultBodyLimit::max(16 * 1024)),
            )
            .route(
                "/oauth/revoke",
                post(mcp_revoke_token).layer(DefaultBodyLimit::max(16 * 1024)),
            );
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
                    let path = request
                        .extensions()
                        .get::<MatchedPath>()
                        .map(MatchedPath::as_str)
                        .unwrap_or("<unmatched>");
                    tracing::info_span!(
                        "http_request",
                        request_id,
                        method = %request.method(),
                        path,
                        problem_code = tracing::field::Empty,
                        note_diagnostic_count = tracing::field::Empty,
                    )
                })
                .on_response(log_http_response)
                .on_failure(()),
        )
        .layer(middleware::from_fn(security_headers))
        .layer(middleware::from_fn(assign_request_id))
}

fn log_http_response(response: &Response, latency: Duration, span: &Span) {
    let status = response.status();
    let outcome = http_outcome(status);
    let latency_ms = u64::try_from(latency.as_millis()).unwrap_or(u64::MAX);
    if status.is_server_error() {
        tracing::error!(
            parent: span,
            event = "http.request.completed",
            outcome,
            status = status.as_u16(),
            latency_ms,
            "HTTP request completed"
        );
    } else {
        tracing::info!(
            parent: span,
            event = "http.request.completed",
            outcome,
            status = status.as_u16(),
            latency_ms,
            "HTTP request completed"
        );
    }
}

fn http_outcome(status: StatusCode) -> &'static str {
    if status.is_server_error() {
        "failure"
    } else if status.is_client_error() {
        "rejected"
    } else {
        "success"
    }
}

async fn openapi() -> Response {
    (
        [(header::CONTENT_TYPE, "application/json; charset=utf-8")],
        OPENAPI_DOCUMENT,
    )
        .into_response()
}

async fn health() -> Json<marginalis_contract::HealthResponse> {
    Json(marginalis_contract::HealthResponse {
        status: "ok".into(),
        api_version: API_VERSION.into(),
    })
}

fn mcp_endpoint(state: &ApiState) -> HandlerResult<&Arc<McpEndpoint>> {
    state.mcp.as_ref().ok_or_else(|| {
        problem(
            StatusCode::NOT_FOUND,
            ProblemCode::NotFound,
            "MCP is not available",
        )
    })
}
