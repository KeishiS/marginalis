//! v0.3.0専用のHTTP APIと早期閲覧UI。
//!
//! 旧`/api/v1`・root管理・ローカル`UserId`を参照しない。composition rootは
//! v0.3.0ではこのrouterだけを公開する。

mod auth;
mod error;
mod html;
mod notes;
mod security;
mod state;
mod ui;

pub use state::{ApiState, McpEndpoint};

use super::{RequestId, assign_request_id};
use std::{sync::Arc, time::Instant};

use crate::mcp::{JsonRpcRequest, JsonRpcResponse, MCP_PROTOCOL_VERSION};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Form, Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    middleware,
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post},
};
use marginalis_application::{McpAuthorizationRequest, McpOAuthUseCaseError};
use marginalis_domain::{Actor, EntityId, Note, NoteDraft, NoteId};
use serde::{Deserialize, Serialize};
use tower_http::trace::{DefaultOnResponse, TraceLayer};
use tracing::{Level, info_span};

#[cfg(test)]
use self::auth::{RETURN_TO_COOKIE, valid_return_to, validate_mutation_origin};
#[cfg(test)]
use self::state::McpRegistrationRateLimiter;
use self::{
    auth::{
        CSRF_COOKIE, authenticated_actor, authenticated_form_actor, authenticated_mutation_actor,
        begin_login, complete_login, cookie_value, external_path, logout, parse_note_id,
    },
    error::{HandlerResult, authentication_error, mcp_error, note_error, problem},
    html::escape_html,
    notes::{
        NoteInput, create_note, delete_note, export_note, list_notes, read_note, restore_note,
        session, update_note,
    },
    security::security_headers,
    ui::{home, view_note},
};
#[cfg(test)]
use marginalis_application::{
    McpOAuthUseCases, NoteUseCases, OidcAuthenticationUseCases, WebSessionUseCases,
};
#[cfg(test)]
use std::time::Duration;

pub const API_VERSION: &str = "v2";
pub const OPENAPI_DOCUMENT: &str = include_str!("../../../docs/openapi.json");
pub fn router(state: ApiState) -> Router {
    let mut router = Router::new()
        .route("/", get(home))
        .route("/notes/{note_id}", get(view_note))
        .route("/api/v2/openapi.json", get(openapi))
        .route("/auth/oidc/login", get(begin_login))
        .route("/auth/oidc/callback", get(complete_login))
        .route("/auth/logout", post(logout))
        .route(
            "/oauth/authorize",
            get(mcp_authorize)
                .post(mcp_authorize_submit)
                .layer(DefaultBodyLimit::max(16 * 1024)),
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
            post(mcp_post).layer(DefaultBodyLimit::max(1024 * 1024)),
        )
        .route("/api/v2/health", get(health))
        .route("/api/v2/session", get(session))
        .route("/api/v2/notes", get(list_notes).post(create_note))
        .route(
            "/api/v2/notes/{note_id}",
            get(read_note).put(update_note).delete(delete_note),
        )
        .route("/api/v2/notes/{note_id}/restore", post(restore_note))
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

async fn mcp_resource_metadata(
    State(state): State<ApiState>,
) -> HandlerResult<Json<serde_json::Value>> {
    let endpoint = mcp_endpoint(&state)?;
    Ok(Json(
        serde_json::json!({"resource": endpoint.resource_uri, "authorization_servers": [endpoint.authorization_server_uri], "bearer_methods_supported": ["header"], "scopes_supported": ["notes:read", "notes:write", "notes:delete"]}),
    ))
}

async fn mcp_server_metadata(
    State(state): State<ApiState>,
) -> HandlerResult<Json<serde_json::Value>> {
    let endpoint = mcp_endpoint(&state)?;
    Ok(Json(
        serde_json::json!({"issuer": endpoint.authorization_server_uri, "authorization_endpoint": endpoint.authorization_endpoint_uri, "token_endpoint": endpoint.token_endpoint_uri, "registration_endpoint": format!("{}/oauth/register", endpoint.authorization_server_uri.trim_end_matches('/')), "response_types_supported": ["code"], "grant_types_supported": ["authorization_code", "refresh_token"], "code_challenge_methods_supported": ["S256"], "token_endpoint_auth_methods_supported": ["none"]}),
    ))
}

#[derive(Deserialize)]
struct McpAuthorizeQuery {
    response_type: String,
    client_id: String,
    redirect_uri: String,
    resource: String,
    scope: String,
    code_challenge: String,
    code_challenge_method: String,
    state: Option<String>,
}
#[derive(Deserialize)]
struct McpAuthorizeForm {
    client_id: String,
    redirect_uri: String,
    resource: String,
    scope: String,
    code_challenge: String,
    state: Option<String>,
    csrf_token: String,
    decision: String,
}
#[derive(Deserialize)]
struct McpTokenForm {
    grant_type: String,
    code: Option<String>,
    client_id: String,
    redirect_uri: Option<String>,
    resource: String,
    code_verifier: Option<String>,
    refresh_token: Option<String>,
}
#[derive(Serialize)]
struct McpTokenResponse {
    access_token: String,
    refresh_token: String,
    token_type: &'static str,
    expires_in: u64,
    scope: String,
}

#[derive(Serialize)]
struct OAuthErrorResponse {
    error: &'static str,
    error_description: &'static str,
}

fn oauth_error_response(
    status: StatusCode,
    error: &'static str,
    description: &'static str,
) -> Response {
    (
        status,
        [
            (header::CACHE_CONTROL, "no-store"),
            (header::PRAGMA, "no-cache"),
        ],
        Json(OAuthErrorResponse {
            error,
            error_description: description,
        }),
    )
        .into_response()
}

#[derive(Deserialize)]
struct McpRegistrationRequest {
    client_name: Option<String>,
    redirect_uris: Vec<String>,
}

#[derive(Serialize)]
struct McpRegistrationResponse {
    client_id: String,
    client_name: String,
    redirect_uris: Vec<String>,
    token_endpoint_auth_method: &'static str,
    grant_types: [&'static str; 2],
    response_types: [&'static str; 1],
}

async fn mcp_register_client(
    State(state): State<ApiState>,
    Json(request): Json<McpRegistrationRequest>,
) -> Result<Response, Response> {
    if !state.mcp_registration_limiter.allow(Instant::now()) {
        return Err(oauth_error_response(
            StatusCode::TOO_MANY_REQUESTS,
            "temporarily_unavailable",
            "dynamic client registration rate limit exceeded",
        ));
    }
    let endpoint = mcp_endpoint(&state).map_err(|error| error.into_response())?;
    let display_name = request
        .client_name
        .filter(|name| !name.trim().is_empty())
        .ok_or_else(|| {
            oauth_error_response(
                StatusCode::BAD_REQUEST,
                "invalid_client_metadata",
                "client_name is required",
            )
        })?;
    let client = marginalis_domain::McpOAuthClient {
        client_id: format!("mcp-{}", uuid::Uuid::now_v7()),
        display_name,
        redirect_uris: request.redirect_uris,
    };
    endpoint
        .oauth
        .register_client(client.clone())
        .await
        .map_err(|error| match error {
            McpOAuthUseCaseError::Rejected => oauth_error_response(
                StatusCode::BAD_REQUEST,
                "invalid_client_metadata",
                "client metadata is invalid",
            ),
            McpOAuthUseCaseError::Unavailable => oauth_error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                "temporarily_unavailable",
                "OAuth service is unavailable",
            ),
        })?;
    Ok((
        StatusCode::CREATED,
        Json(McpRegistrationResponse {
            client_id: client.client_id,
            client_name: client.display_name,
            redirect_uris: client.redirect_uris,
            token_endpoint_auth_method: "none",
            grant_types: ["authorization_code", "refresh_token"],
            response_types: ["code"],
        }),
    )
        .into_response())
}

fn authorize_fields(query: &McpAuthorizeQuery) -> HandlerResult<(Vec<String>, String)> {
    if query.response_type != "code"
        || query.code_challenge_method != "S256"
        || query.code_challenge.is_empty()
    {
        return Err(problem(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "OAuth request is invalid",
        ));
    }
    let scopes = query
        .scope
        .split_ascii_whitespace()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if scopes.is_empty() {
        return Err(problem(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "OAuth request is invalid",
        ));
    }
    Ok((scopes, query.code_challenge.clone()))
}

fn authorization_request(query: &McpAuthorizeQuery) -> HandlerResult<McpAuthorizationRequest> {
    let (scopes, code_challenge) = authorize_fields(query)?;
    Ok(McpAuthorizationRequest {
        client_id: query.client_id.clone(),
        redirect_uri: query.redirect_uri.clone(),
        resource_uri: query.resource.clone(),
        scopes,
        code_challenge,
    })
}

async fn mcp_authorize(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Query(query): Query<McpAuthorizeQuery>,
) -> HandlerResult<Response> {
    let endpoint = mcp_endpoint(&state)?;
    let request = authorization_request(&query)?;
    let client = endpoint
        .oauth
        .validate_authorization_request(request)
        .await
        .map_err(mcp_error)?;
    let _actor = match authenticated_actor(&headers, &state).await {
        Ok(actor) => actor,
        Err((StatusCode::UNAUTHORIZED, _)) => {
            let mut request_uri = url::Url::parse("https://invalid.example/oauth/authorize")
                .expect("constant authorization URL is valid");
            {
                let mut pairs = request_uri.query_pairs_mut();
                pairs.append_pair("response_type", &query.response_type);
                pairs.append_pair("client_id", &query.client_id);
                pairs.append_pair("redirect_uri", &query.redirect_uri);
                pairs.append_pair("resource", &query.resource);
                pairs.append_pair("scope", &query.scope);
                pairs.append_pair("code_challenge", &query.code_challenge);
                pairs.append_pair("code_challenge_method", &query.code_challenge_method);
                if let Some(state) = &query.state {
                    pairs.append_pair("state", state);
                }
            }
            let next = format!(
                "{}?{}",
                external_path(&state.cookie_path, "/oauth/authorize"),
                request_uri.query().expect("query pairs were added")
            );
            let encoded_next =
                url::form_urlencoded::byte_serialize(next.as_bytes()).collect::<String>();
            return Ok(Redirect::temporary(&format!(
                "{}?next={encoded_next}",
                external_path(&state.cookie_path, "/auth/oidc/login")
            ))
            .into_response());
        }
        Err(error) => return Err(error),
    };
    let csrf = cookie_value(&headers, CSRF_COOKIE).ok_or_else(|| {
        problem(
            StatusCode::FORBIDDEN,
            "csrf_required",
            "CSRF token is required",
        )
    })?;
    Ok(Html(format!(
        "<!doctype html><meta charset=\"utf-8\"><title>MCP authorization</title><main><h1>Authorize {}</h1><p>Requested scopes: {}</p><p>Redirect host: {}</p><form method=\"post\"><input type=\"hidden\" name=\"client_id\" value=\"{}\"><input type=\"hidden\" name=\"redirect_uri\" value=\"{}\"><input type=\"hidden\" name=\"resource\" value=\"{}\"><input type=\"hidden\" name=\"scope\" value=\"{}\"><input type=\"hidden\" name=\"code_challenge\" value=\"{}\"><input type=\"hidden\" name=\"state\" value=\"{}\"><input type=\"hidden\" name=\"csrf_token\" value=\"{}\"><button name=\"decision\" value=\"approve\">Allow</button><button name=\"decision\" value=\"deny\">Deny</button></form></main>",
        escape_html(&client.display_name),
        escape_html(&query.scope),
        escape_html(url::Url::parse(&query.redirect_uri).ok().and_then(|url| url.host_str().map(str::to_owned)).as_deref().unwrap_or("unknown")),
        escape_html(&query.client_id),
        escape_html(&query.redirect_uri),
        escape_html(&query.resource),
        escape_html(&query.scope),
        escape_html(&query.code_challenge),
        escape_html(query.state.as_deref().unwrap_or_default()),
        escape_html(&csrf)
    )).into_response())
}

async fn mcp_authorize_submit(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Form(form): Form<McpAuthorizeForm>,
) -> HandlerResult<Response> {
    let endpoint = mcp_endpoint(&state)?;
    let actor = authenticated_form_actor(&headers, &state, &form.csrf_token).await?;
    let request = McpAuthorizationRequest {
        client_id: form.client_id,
        redirect_uri: form.redirect_uri,
        resource_uri: form.resource,
        scopes: form
            .scope
            .split_ascii_whitespace()
            .map(str::to_owned)
            .collect(),
        code_challenge: form.code_challenge,
    };
    endpoint
        .oauth
        .validate_authorization_request(request.clone())
        .await
        .map_err(mcp_error)?;
    if form.decision != "approve" {
        return oauth_redirect(
            &request.redirect_uri,
            form.state.as_deref(),
            None,
            Some("access_denied"),
        );
    }
    let code = endpoint
        .oauth
        .authorize(actor, request.clone())
        .await
        .map_err(mcp_error)?;
    oauth_redirect(
        &request.redirect_uri,
        form.state.as_deref(),
        Some(&code),
        None,
    )
}

fn oauth_redirect(
    redirect_uri: &str,
    state: Option<&str>,
    code: Option<&str>,
    error: Option<&str>,
) -> HandlerResult<Response> {
    let mut url = url::Url::parse(redirect_uri).map_err(|_| {
        problem(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "redirect URI is invalid",
        )
    })?;
    {
        let mut pairs = url.query_pairs_mut();
        if let Some(code) = code {
            pairs.append_pair("code", code);
        }
        if let Some(error) = error {
            pairs.append_pair("error", error);
        }
        if let Some(state) = state {
            pairs.append_pair("state", state);
        }
    }
    Ok(Redirect::to(url.as_str()).into_response())
}

async fn mcp_token(
    State(state): State<ApiState>,
    Form(form): Form<McpTokenForm>,
) -> Result<Response, Response> {
    let endpoint = mcp_endpoint(&state).map_err(|_| {
        oauth_error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "temporarily_unavailable",
            "OAuth service is unavailable",
        )
    })?;
    let pair = match form.grant_type.as_str() {
        "authorization_code" => endpoint
            .oauth
            .exchange_authorization_code(
                form.code.ok_or_else(|| {
                    oauth_error_response(
                        StatusCode::BAD_REQUEST,
                        "invalid_request",
                        "code is required",
                    )
                })?,
                form.client_id,
                form.redirect_uri.ok_or_else(|| {
                    oauth_error_response(
                        StatusCode::BAD_REQUEST,
                        "invalid_request",
                        "redirect_uri is required",
                    )
                })?,
                form.resource,
                form.code_verifier.ok_or_else(|| {
                    oauth_error_response(
                        StatusCode::BAD_REQUEST,
                        "invalid_request",
                        "code_verifier is required",
                    )
                })?,
            )
            .await
            .map_err(|error| match error {
                McpOAuthUseCaseError::Rejected => oauth_error_response(
                    StatusCode::BAD_REQUEST,
                    "invalid_grant",
                    "authorization code is invalid",
                ),
                McpOAuthUseCaseError::Unavailable => oauth_error_response(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "temporarily_unavailable",
                    "OAuth service is unavailable",
                ),
            })?,
        "refresh_token" => endpoint
            .oauth
            .refresh_access_token(
                form.refresh_token.ok_or_else(|| {
                    oauth_error_response(
                        StatusCode::BAD_REQUEST,
                        "invalid_request",
                        "refresh_token is required",
                    )
                })?,
                form.client_id,
                form.resource,
            )
            .await
            .map_err(|error| match error {
                McpOAuthUseCaseError::Rejected => oauth_error_response(
                    StatusCode::BAD_REQUEST,
                    "invalid_grant",
                    "refresh token is invalid",
                ),
                McpOAuthUseCaseError::Unavailable => oauth_error_response(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "temporarily_unavailable",
                    "OAuth service is unavailable",
                ),
            })?,
        _ => {
            return Err(oauth_error_response(
                StatusCode::BAD_REQUEST,
                "unsupported_grant_type",
                "OAuth grant type is unsupported",
            ));
        }
    };
    Ok((
        [
            (header::CACHE_CONTROL, "no-store"),
            (header::PRAGMA, "no-cache"),
        ],
        Json(McpTokenResponse {
            access_token: pair.access_token,
            refresh_token: pair.refresh_token,
            token_type: "Bearer",
            expires_in: pair.access_expires_in_seconds,
            scope: pair.scope,
        }),
    )
        .into_response())
}

async fn revoke_mcp_authorization(
    State(state): State<ApiState>,
    Path(client_id): Path<String>,
    headers: HeaderMap,
) -> HandlerResult<StatusCode> {
    let actor = authenticated_mutation_actor(&headers, &state).await?;
    let endpoint = mcp_endpoint(&state)?;
    if client_id.trim().is_empty() {
        return Err(problem(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "client ID is invalid",
        ));
    }
    endpoint
        .oauth
        .revoke(actor, client_id)
        .await
        .map_err(mcp_error)?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct McpToolCall {
    name: String,
    arguments: serde_json::Value,
}

fn mcp_required_scope(request: &JsonRpcRequest) -> &'static str {
    if request.method != "tools/call" {
        return "notes:read";
    }
    serde_json::from_value::<McpToolCall>(request.params.clone().unwrap_or_default())
        .ok()
        .map(|call| match call.name.as_str() {
            "create_note" | "update_note" => "notes:write",
            "delete_note" => "notes:delete",
            _ => "notes:read",
        })
        .unwrap_or("notes:read")
}

fn mcp_unauthorized(endpoint: &McpEndpoint) -> Response {
    let mut response = StatusCode::UNAUTHORIZED.into_response();
    if let Ok(value) = format!("Bearer resource_metadata=\"{}\"", endpoint.metadata_uri).parse() {
        response
            .headers_mut()
            .insert(header::WWW_AUTHENTICATE, value);
    }
    response
}

async fn mcp_post(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(request): Json<JsonRpcRequest>,
) -> HandlerResult<Response> {
    let endpoint = mcp_endpoint(&state)?;
    if let Some(value) = headers.get(header::ORIGIN) {
        let origin = value.to_str().map_err(|_| {
            problem(
                StatusCode::FORBIDDEN,
                "origin_not_allowed",
                "MCP browser request origin is not allowed",
            )
        })?;
        if origin != state.browser_origin
            && !endpoint
                .allowed_origins
                .iter()
                .any(|allowed| allowed == origin)
        {
            tracing::warn!(
                received_origin = origin,
                "rejected MCP browser request from an untrusted origin"
            );
            return Err(problem(
                StatusCode::FORBIDDEN,
                "origin_not_allowed",
                "MCP browser request origin is not allowed",
            ));
        }
    }
    let accepts = headers
        .get(header::ACCEPT)
        .and_then(|value| value.to_str().ok())
        .map(|value| {
            value
                .split(',')
                .map(|item| item.trim().split(';').next().unwrap_or_default())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if !accepts.contains(&"application/json") || !accepts.contains(&"text/event-stream") {
        return Ok(StatusCode::NOT_ACCEPTABLE.into_response());
    }
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));
    let Some(token) = token else {
        return Ok(mcp_unauthorized(endpoint));
    };
    let Some(actor) = endpoint
        .oauth
        .authenticate(
            token.into(),
            endpoint.resource_uri.clone(),
            mcp_required_scope(&request).into(),
        )
        .await
        .map_err(mcp_error)?
        .map(|value| value.actor)
    else {
        return Ok(mcp_unauthorized(endpoint));
    };
    let id = request.id.clone().unwrap_or(serde_json::Value::Null);
    if request.jsonrpc != "2.0" {
        return if request.id.is_none() {
            Ok(StatusCode::ACCEPTED.into_response())
        } else {
            Ok(Json(JsonRpcResponse::error(
                id,
                -32600,
                "JSON-RPC version is invalid",
            ))
            .into_response())
        };
    }
    let response = match request.method.as_str() {
        "initialize" => JsonRpcResponse::success(
            id,
            serde_json::json!({"protocolVersion": MCP_PROTOCOL_VERSION, "capabilities":{"tools":{}}, "serverInfo":{"name":"marginalis","version":env!("CARGO_PKG_VERSION")}}),
        ),
        "tools/list" => JsonRpcResponse::success(
            id,
            serde_json::json!({"tools":[
                {"name":"list_notes","description":"List notes visible to the authenticated user.","inputSchema":{"type":"object","properties":{}}},
                {"name":"get_note","description":"Read one visible note.","inputSchema":{"type":"object","required":["note_id"],"properties":{"note_id":{"type":"string"}}}},
                {"name":"create_note","description":"Create a note.","inputSchema":{"type":"object","required":["title","body","tags"],"properties":{"title":{"type":"string"},"body":{"type":"string"},"tags":{"type":"array","items":{"type":"string"}}}}},
                {"name":"update_note","description":"Update a note at its current revision.","inputSchema":{"type":"object","required":["note_id","title","body","tags","expected_revision"],"properties":{"note_id":{"type":"string"},"title":{"type":"string"},"body":{"type":"string"},"tags":{"type":"array","items":{"type":"string"}},"expected_revision":{"type":"integer"}}}},
                {"name":"delete_note","description":"Soft-delete a note at its current revision.","inputSchema":{"type":"object","required":["note_id","expected_revision"],"properties":{"note_id":{"type":"string"},"expected_revision":{"type":"integer"}}}}
            ]}),
        ),
        "tools/call" => mcp_tool_call(endpoint, actor, id, request.params).await,
        _ => JsonRpcResponse::error(id, -32601, "method not found"),
    };
    if request.id.is_none() {
        Ok(StatusCode::ACCEPTED.into_response())
    } else {
        Ok(Json(response).into_response())
    }
}

#[derive(Deserialize)]
struct McpUpdate {
    note_id: String,
    title: String,
    body: String,
    tags: Vec<String>,
    expected_revision: i64,
}
#[derive(Deserialize)]
struct McpDelete {
    note_id: String,
    expected_revision: i64,
}

async fn mcp_tool_call(
    endpoint: &McpEndpoint,
    actor: Actor,
    id: serde_json::Value,
    params: Option<serde_json::Value>,
) -> JsonRpcResponse {
    let Ok(call) = serde_json::from_value::<McpToolCall>(params.unwrap_or_default()) else {
        return JsonRpcResponse::error(id, -32602, "tool parameters are invalid");
    };
    let result = match call.name.as_str() {
        "list_notes" => endpoint.notes.list_visible_notes(actor).await.map(|notes| {
            serde_json::json!(
                notes
                    .into_iter()
                    .map(|note| serde_json::json!({
                        "note_id": note.note_id.to_string(),
                        "title": note.title,
                        "revision": note.revision,
                    }))
                    .collect::<Vec<_>>()
            )
        }),
        "get_note" => {
            let Some(note_id) = mcp_note_id(&call.arguments) else {
                return JsonRpcResponse::error(id, -32602, "note_id is invalid");
            };
            endpoint.notes.read_note(actor, note_id).await.map(|note| {
                serde_json::json!({
                    "note_id": note.note_id.to_string(),
                    "title": note.title,
                    "body": note.body,
                    "tags": note.tags,
                    "revision": note.revision,
                })
            })
        }
        "create_note" => {
            let Ok(input) = serde_json::from_value::<NoteInput>(call.arguments) else {
                return JsonRpcResponse::error(id, -32602, "note arguments are invalid");
            };
            endpoint
                .notes
                .create_note(
                    actor,
                    NoteDraft {
                        title: input.title,
                        body: input.body,
                        tags: input.tags,
                    },
                )
                .await
                .map(note_revision_json)
        }
        "update_note" => {
            let Ok(input) = serde_json::from_value::<McpUpdate>(call.arguments) else {
                return JsonRpcResponse::error(id, -32602, "update arguments are invalid");
            };
            let Some(note_id) = parse_note_id(&input.note_id).ok() else {
                return JsonRpcResponse::error(id, -32602, "note_id is invalid");
            };
            endpoint
                .notes
                .update_note(
                    actor,
                    note_id,
                    NoteDraft {
                        title: input.title,
                        body: input.body,
                        tags: input.tags,
                    },
                    input.expected_revision,
                )
                .await
                .map(note_revision_json)
        }
        "delete_note" => {
            let Ok(input) = serde_json::from_value::<McpDelete>(call.arguments) else {
                return JsonRpcResponse::error(id, -32602, "delete arguments are invalid");
            };
            let Some(note_id) = parse_note_id(&input.note_id).ok() else {
                return JsonRpcResponse::error(id, -32602, "note_id is invalid");
            };
            endpoint
                .notes
                .soft_delete_note(actor, note_id, input.expected_revision)
                .await
                .map(note_revision_json)
        }
        _ => return JsonRpcResponse::error(id, -32601, "tool not found"),
    };
    match result {
        Ok(value) => JsonRpcResponse::success(
            id,
            serde_json::json!({"content":[{"type":"text","text":serde_json::to_string(&value).unwrap_or_default()}],"structuredContent":value}),
        ),
        Err(_) => JsonRpcResponse::error(id, -32000, "note operation failed"),
    }
}

fn mcp_note_id(arguments: &serde_json::Value) -> Option<NoteId> {
    arguments
        .get("note_id")
        .and_then(serde_json::Value::as_str)
        .and_then(|value| parse_note_id(value).ok())
}

fn note_revision_json(note: Note) -> serde_json::Value {
    serde_json::json!({
        "note_id": note.note_id.to_string(),
        "revision": note.revision,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use axum::{body::Body, http::Request};
    use marginalis_application::{
        AuthenticationUseCaseError, McpOAuthUseCaseError, McpTokenPair, NoteUseCaseError,
    };
    use marginalis_domain::{
        AuthenticatedSession, McpAuthenticatedActor, McpOAuthClient, WebSession,
    };
    use tower::ServiceExt;

    struct Notes;

    #[async_trait]
    impl NoteUseCases for Notes {
        async fn list_visible_notes(&self, _actor: Actor) -> Result<Vec<Note>, NoteUseCaseError> {
            Ok(Vec::new())
        }

        async fn read_note(
            &self,
            _actor: Actor,
            _note_id: NoteId,
        ) -> Result<Note, NoteUseCaseError> {
            Err(NoteUseCaseError::NotFound)
        }

        async fn create_note(
            &self,
            _actor: Actor,
            _draft: NoteDraft,
        ) -> Result<Note, NoteUseCaseError> {
            Err(NoteUseCaseError::Unavailable)
        }

        async fn update_note(
            &self,
            _actor: Actor,
            _note_id: NoteId,
            _draft: NoteDraft,
            _expected_revision: i64,
        ) -> Result<Note, NoteUseCaseError> {
            Err(NoteUseCaseError::Unavailable)
        }

        async fn soft_delete_note(
            &self,
            _actor: Actor,
            _note_id: NoteId,
            _expected_revision: i64,
        ) -> Result<Note, NoteUseCaseError> {
            Err(NoteUseCaseError::Unavailable)
        }

        async fn restore_note(
            &self,
            _actor: Actor,
            _note_id: NoteId,
            _expected_revision: i64,
        ) -> Result<Note, NoteUseCaseError> {
            Err(NoteUseCaseError::Unavailable)
        }

        fn export_note_source(&self, _note: &Note) -> Result<String, NoteUseCaseError> {
            Err(NoteUseCaseError::Unavailable)
        }

        fn render_note_html(&self, _note: &Note) -> Result<String, NoteUseCaseError> {
            Err(NoteUseCaseError::Unavailable)
        }
    }

    struct Sessions;

    #[async_trait]
    impl WebSessionUseCases for Sessions {
        async fn authenticate_session(
            &self,
            _session_id: String,
        ) -> Result<Option<AuthenticatedSession>, AuthenticationUseCaseError> {
            Ok(None)
        }

        async fn verify_csrf(
            &self,
            _session_id: String,
            _csrf_token: String,
        ) -> Result<bool, AuthenticationUseCaseError> {
            Ok(false)
        }

        async fn issue_session(
            &self,
            _actor: Actor,
        ) -> Result<WebSession, AuthenticationUseCaseError> {
            Err(AuthenticationUseCaseError::Unavailable)
        }

        async fn revoke_session(
            &self,
            _session_id: String,
        ) -> Result<(), AuthenticationUseCaseError> {
            Ok(())
        }
    }

    struct Oidc;

    #[async_trait]
    impl OidcAuthenticationUseCases for Oidc {
        async fn begin_login(&self) -> Result<String, AuthenticationUseCaseError> {
            Ok("https://id.example.test/authorize".into())
        }

        async fn complete_login(
            &self,
            _code: String,
            _state: String,
        ) -> Result<Actor, AuthenticationUseCaseError> {
            Err(AuthenticationUseCaseError::Unavailable)
        }
    }

    struct Mcp;
    #[async_trait]
    impl McpOAuthUseCases for Mcp {
        async fn register_client(
            &self,
            _client: McpOAuthClient,
        ) -> Result<(), McpOAuthUseCaseError> {
            Ok(())
        }
        async fn validate_authorization_request(
            &self,
            request: marginalis_application::McpAuthorizationRequest,
        ) -> Result<McpOAuthClient, McpOAuthUseCaseError> {
            Ok(McpOAuthClient {
                client_id: request.client_id,
                display_name: "Test MCP client".into(),
                redirect_uris: vec![request.redirect_uri],
            })
        }
        async fn authorize(
            &self,
            _actor: Actor,
            _request: marginalis_application::McpAuthorizationRequest,
        ) -> Result<String, McpOAuthUseCaseError> {
            Err(McpOAuthUseCaseError::Rejected)
        }
        async fn exchange_authorization_code(
            &self,
            _code: String,
            _client_id: String,
            _redirect_uri: String,
            _resource_uri: String,
            _verifier: String,
        ) -> Result<McpTokenPair, McpOAuthUseCaseError> {
            Ok(McpTokenPair {
                access_token: "access-token".into(),
                refresh_token: "refresh-token".into(),
                access_expires_in_seconds: 300,
                scope: "notes:read".into(),
            })
        }
        async fn refresh_access_token(
            &self,
            _refresh_token: String,
            _client_id: String,
            _resource_uri: String,
        ) -> Result<McpTokenPair, McpOAuthUseCaseError> {
            Err(McpOAuthUseCaseError::Rejected)
        }
        async fn authenticate(
            &self,
            token: String,
            _resource_uri: String,
            _scope: String,
        ) -> Result<Option<McpAuthenticatedActor>, McpOAuthUseCaseError> {
            Ok((token == "valid-token").then(|| McpAuthenticatedActor {
                actor: Actor {
                    issuer: "https://kanidm.example.test".into(),
                    subject: "alice".into(),
                    is_administrator: false,
                },
            }))
        }
        async fn revoke(
            &self,
            _actor: Actor,
            _client_id: String,
        ) -> Result<(), McpOAuthUseCaseError> {
            Ok(())
        }
    }

    fn app() -> Router {
        router(ApiState::new(
            Arc::new(Notes),
            Arc::new(Sessions),
            Arc::new(Oidc),
            "/".into(),
            "https://example.test".into(),
        ))
    }

    fn mcp_app() -> Router {
        router(
            ApiState::new(
                Arc::new(Notes),
                Arc::new(Sessions),
                Arc::new(Oidc),
                "/".into(),
                "https://example.test".into(),
            )
            .with_mcp(McpEndpoint {
                oauth: Arc::new(Mcp),
                notes: Arc::new(Notes),
                allowed_origins: vec!["https://chatgpt.com".into()],
                resource_uri: "https://example.test/mcp".into(),
                metadata_uri: "https://example.test/.well-known/oauth-protected-resource/mcp"
                    .into(),
                authorization_server_uri: "https://example.test".into(),
                authorization_server_metadata_uri:
                    "https://example.test/.well-known/oauth-authorization-server".into(),
                authorization_endpoint_uri: "https://example.test/oauth/authorize".into(),
                token_endpoint_uri: "https://example.test/oauth/token".into(),
            }),
        )
    }

    fn subpath_mcp_app() -> Router {
        router(
            ApiState::new(
                Arc::new(Notes),
                Arc::new(Sessions),
                Arc::new(Oidc),
                "/marginalis".into(),
                "https://example.test".into(),
            )
            .with_mcp(McpEndpoint {
                oauth: Arc::new(Mcp),
                notes: Arc::new(Notes),
                allowed_origins: vec![],
                resource_uri: "https://example.test/marginalis/mcp".into(),
                metadata_uri:
                    "https://example.test/.well-known/oauth-protected-resource/marginalis/mcp"
                        .into(),
                authorization_server_uri: "https://example.test/marginalis".into(),
                authorization_server_metadata_uri:
                    "https://example.test/.well-known/oauth-authorization-server/marginalis".into(),
                authorization_endpoint_uri: "https://example.test/marginalis/oauth/authorize"
                    .into(),
                token_endpoint_uri: "https://example.test/marginalis/oauth/token".into(),
            }),
        )
    }

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
                Request::get("/api/v2/health")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(health.status(), StatusCode::OK);
        let request_id = health
            .headers()
            .get(super::super::REQUEST_ID_HEADER)
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
                Request::get("/api/v2/notes")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(notes.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn openapi_is_served_from_the_embedded_contract() {
        let response = app()
            .oneshot(
                Request::get("/api/v2/openapi.json")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(OPENAPI_DOCUMENT, include_str!("../../../docs/openapi.json"));
    }

    #[tokio::test]
    async fn mcp_metadata_is_available_when_enabled() {
        let response = mcp_app()
            .oneshot(
                Request::get("/.well-known/oauth-authorization-server")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn mcp_metadata_uses_rfc_well_known_paths_for_a_subpath_issuer() {
        for path in [
            "/.well-known/oauth-protected-resource/marginalis/mcp",
            "/.well-known/oauth-authorization-server/marginalis",
        ] {
            let response = subpath_mcp_app()
                .oneshot(Request::get(path).body(Body::empty()).expect("request"))
                .await
                .expect("response");
            assert_eq!(response.status(), StatusCode::OK, "{path}");
        }

        let non_standard = subpath_mcp_app()
            .oneshot(
                Request::get("/marginalis/.well-known/oauth-authorization-server")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(non_standard.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn mcp_authorization_starts_login_when_no_web_session_exists() {
        let response = mcp_app()
            .oneshot(
                Request::get(
                    "/oauth/authorize?response_type=code&client_id=client&redirect_uri=http%3A%2F%2F127.0.0.1%3A48123%2Fcallback&resource=https%3A%2F%2Fexample.test%2Fmcp&scope=notes%3Aread&code_challenge=verifier&code_challenge_method=S256&state=opaque",
                )
                .body(Body::empty())
                .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::TEMPORARY_REDIRECT);
        let login_location = response
            .headers()
            .get(header::LOCATION)
            .expect("login location")
            .to_str()
            .expect("valid location")
            .to_owned();
        assert!(login_location.starts_with("/auth/oidc/login?next="));

        let login = mcp_app()
            .oneshot(
                Request::get(login_location)
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(login.status(), StatusCode::TEMPORARY_REDIRECT);
        assert_eq!(
            login.headers().get(header::LOCATION).expect("location"),
            "https://id.example.test/authorize"
        );
        assert!(
            login
                .headers()
                .get_all(header::SET_COOKIE)
                .iter()
                .any(|value| value
                    .to_str()
                    .is_ok_and(|value| value.contains(RETURN_TO_COOKIE)))
        );
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
    }

    #[tokio::test]
    async fn mcp_accepts_configured_browser_origins_and_rejects_others() {
        let request = Request::post("/mcp")
            .header("content-type", "application/json")
            .header(header::ACCEPT, "application/json, text/event-stream")
            .header(header::ORIGIN, "https://chatgpt.com")
            .body(Body::from(
                r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#,
            ))
            .expect("request");
        let allowed = mcp_app().oneshot(request).await.expect("response");
        assert_eq!(allowed.status(), StatusCode::UNAUTHORIZED);

        let request = Request::post("/mcp")
            .header("content-type", "application/json")
            .header(header::ACCEPT, "application/json, text/event-stream")
            .header(header::ORIGIN, "https://example.test")
            .body(Body::from(
                r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#,
            ))
            .expect("request");
        let same_origin = mcp_app().oneshot(request).await.expect("response");
        assert_eq!(same_origin.status(), StatusCode::UNAUTHORIZED);

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
    }

    #[test]
    fn mcp_registration_limiter_bounds_a_window() {
        let limiter = McpRegistrationRateLimiter::new(1, Duration::from_secs(60));
        let now = Instant::now();
        assert!(limiter.allow(now));
        assert!(!limiter.allow(now));
        assert!(limiter.allow(now + Duration::from_secs(61)));
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

    #[tokio::test]
    async fn mcp_dynamic_registration_creates_a_public_client() {
        let response = mcp_app()
            .oneshot(
                Request::post("/oauth/register")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"client_name":"Codex CLI","redirect_uris":["http://127.0.0.1:48123/callback"]}"#,
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::CREATED);
    }

    #[tokio::test]
    async fn mcp_token_response_is_not_cacheable() {
        let response = mcp_app()
            .oneshot(
                Request::post("/oauth/token")
                    .header("content-type", "application/x-www-form-urlencoded")
                    .body(Body::from(
                        "grant_type=authorization_code&code=code&client_id=client&redirect_uri=http%3A%2F%2F127.0.0.1%2Fcallback&resource=https%3A%2F%2Fexample.test%2Fmcp&code_verifier=verifier",
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CACHE_CONTROL),
            Some(&"no-store".parse().expect("header"))
        );
        assert_eq!(
            response.headers().get(header::PRAGMA),
            Some(&"no-cache".parse().expect("header"))
        );
    }

    #[tokio::test]
    async fn mcp_token_errors_use_oauth_error_shape() {
        let response = mcp_app()
            .oneshot(
                Request::post("/oauth/token")
                    .header("content-type", "application/x-www-form-urlencoded")
                    .body(Body::from(
                        "grant_type=password&client_id=client&resource=https%3A%2F%2Fexample.test%2Fmcp",
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            response.headers().get(header::CACHE_CONTROL),
            Some(&"no-store".parse().expect("header"))
        );
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let error: serde_json::Value = serde_json::from_slice(&body).expect("OAuth error");
        assert_eq!(error["error"], "unsupported_grant_type");
    }

    #[tokio::test]
    async fn public_mcp_endpoints_reject_oversized_request_bodies() {
        let registration = mcp_app()
            .oneshot(
                Request::post("/oauth/register")
                    .header("content-type", "application/json")
                    .body(Body::from(vec![b' '; 16 * 1024 + 1]))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(registration.status(), StatusCode::PAYLOAD_TOO_LARGE);

        let mcp = mcp_app()
            .oneshot(
                Request::post("/mcp")
                    .header("content-type", "application/json")
                    .body(Body::from(vec![b' '; 1024 * 1024 + 1]))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(mcp.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }
}
