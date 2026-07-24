//! v0.3.0専用のHTTP APIと早期閲覧UI。
//!
//! このmoduleはv0.2の`/api/v1`・root管理・ローカル`UserId`を参照しない。composition rootは
//! v0.3.0ではこのrouterだけを公開する。

use std::{str::FromStr, sync::Arc};

use axum::{
    Json, Router,
    extract::{Form, Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post},
};
use marginalis_application::{
    AuthenticationUseCaseError, McpOAuthUseCaseError, NoteUseCaseError, V3McpOAuthUseCases,
    V3NoteUseCases, V3OidcAuthenticationUseCases, V3WebSessionUseCases,
};
use marginalis_domain::{CanonicalActor, CanonicalNote, CanonicalNoteDraft, EntityId, NoteId};
use serde::{Deserialize, Serialize};

pub const API_VERSION: &str = "v2";
pub const OPENAPI_DOCUMENT: &str = include_str!("../../../docs/openapi-v3.json");
const SESSION_COOKIE: &str = "marginalis_session";
const CSRF_COOKIE: &str = "marginalis_csrf";

#[derive(Clone)]
pub struct V3ApiState {
    pub notes: Arc<dyn V3NoteUseCases>,
    pub sessions: Arc<dyn V3WebSessionUseCases>,
    pub oidc: Arc<dyn V3OidcAuthenticationUseCases>,
    pub cookie_path: String,
    pub browser_origin: String,
    pub mcp: Option<Arc<V3McpEndpoint>>,
}

pub struct V3McpEndpoint {
    pub oauth: Arc<dyn V3McpOAuthUseCases>,
    pub resource_uri: String,
    pub metadata_uri: String,
    pub authorization_server_uri: String,
    pub authorization_endpoint_uri: String,
    pub token_endpoint_uri: String,
}

impl V3ApiState {
    pub fn new(
        notes: Arc<dyn V3NoteUseCases>,
        sessions: Arc<dyn V3WebSessionUseCases>,
        oidc: Arc<dyn V3OidcAuthenticationUseCases>,
        cookie_path: String,
        browser_origin: String,
    ) -> Self {
        Self {
            notes,
            sessions,
            oidc,
            cookie_path,
            browser_origin,
            mcp: None,
        }
    }

    pub fn with_mcp(mut self, mcp: V3McpEndpoint) -> Self {
        self.mcp = Some(Arc::new(mcp));
        self
    }
}

#[derive(Serialize)]
struct Problem {
    code: &'static str,
    message: &'static str,
}

type V3Result<T> = Result<T, (StatusCode, Json<Problem>)>;

fn problem(
    status: StatusCode,
    code: &'static str,
    message: &'static str,
) -> (StatusCode, Json<Problem>) {
    (status, Json(Problem { code, message }))
}

fn note_error(error: NoteUseCaseError) -> (StatusCode, Json<Problem>) {
    match error {
        NoteUseCaseError::NotFound => {
            problem(StatusCode::NOT_FOUND, "not_found", "note is not available")
        }
        NoteUseCaseError::Forbidden => problem(
            StatusCode::FORBIDDEN,
            "forbidden",
            "note operation is not permitted",
        ),
        NoteUseCaseError::Conflict => {
            problem(StatusCode::CONFLICT, "conflict", "note revision conflicts")
        }
        NoteUseCaseError::Validation => problem(
            StatusCode::UNPROCESSABLE_ENTITY,
            "validation_failed",
            "note is invalid",
        ),
        NoteUseCaseError::Unavailable => problem(
            StatusCode::SERVICE_UNAVAILABLE,
            "unavailable",
            "note operation is unavailable",
        ),
    }
}

fn authentication_error(error: AuthenticationUseCaseError) -> (StatusCode, Json<Problem>) {
    match error {
        AuthenticationUseCaseError::Rejected | AuthenticationUseCaseError::NotFound => problem(
            StatusCode::UNAUTHORIZED,
            "authentication_required",
            "authentication is required",
        ),
        AuthenticationUseCaseError::Unavailable => problem(
            StatusCode::SERVICE_UNAVAILABLE,
            "authentication_unavailable",
            "authentication is unavailable",
        ),
    }
}

#[derive(Serialize)]
struct SessionResponse {
    issuer: String,
    subject: String,
    is_administrator: bool,
}

#[derive(Serialize)]
struct NoteResponse {
    note_id: String,
    title: String,
    body: String,
    tags: Vec<String>,
    created_at_ms: i64,
    updated_at_ms: i64,
    revision: i64,
}

impl From<CanonicalNote> for NoteResponse {
    fn from(note: CanonicalNote) -> Self {
        Self {
            note_id: note.note_id.to_string(),
            title: note.title,
            body: note.body,
            tags: note.tags,
            created_at_ms: note.created_at.get(),
            updated_at_ms: note.updated_at.get(),
            revision: note.revision,
        }
    }
}

#[derive(Deserialize)]
struct NoteInput {
    title: String,
    body: String,
    tags: Vec<String>,
}

#[derive(Deserialize)]
struct NoteUpdateInput {
    title: String,
    body: String,
    tags: Vec<String>,
    expected_revision: i64,
}

#[derive(Deserialize)]
struct DeleteInput {
    expected_revision: i64,
}

pub fn router(state: V3ApiState) -> Router {
    Router::new()
        .route("/", get(home))
        .route("/notes/{note_id}", get(view_note))
        .route("/api/v2/openapi.json", get(openapi))
        .route("/auth/oidc/login", get(begin_login))
        .route("/auth/oidc/callback", get(complete_login))
        .route("/auth/logout", post(logout))
        .route(
            "/.well-known/oauth-protected-resource/mcp",
            get(mcp_resource_metadata),
        )
        .route(
            "/.well-known/oauth-authorization-server",
            get(mcp_server_metadata),
        )
        .route(
            "/oauth/authorize",
            get(mcp_authorize).post(mcp_authorize_submit),
        )
        .route("/oauth/token", post(mcp_token))
        .route("/api/v2/health", get(health))
        .route("/api/v2/session", get(session))
        .route("/api/v2/notes", get(list_notes).post(create_note))
        .route(
            "/api/v2/notes/{note_id}",
            get(read_note).put(update_note).delete(delete_note),
        )
        .route("/api/v2/notes/{note_id}/restore", post(restore_note))
        .route("/api/v2/notes/{note_id}/source", get(export_note))
        .with_state(state)
}

async fn openapi() -> Response {
    (
        [(header::CONTENT_TYPE, "application/json; charset=utf-8")],
        OPENAPI_DOCUMENT,
    )
        .into_response()
}

#[derive(Deserialize)]
struct OidcCallbackQuery {
    code: String,
    state: String,
}

async fn begin_login(State(state): State<V3ApiState>) -> V3Result<Redirect> {
    Ok(Redirect::temporary(
        &state
            .oidc
            .begin_login()
            .await
            .map_err(authentication_error)?,
    ))
}

async fn complete_login(
    State(state): State<V3ApiState>,
    axum::extract::Query(query): axum::extract::Query<OidcCallbackQuery>,
) -> V3Result<Response> {
    let actor = state
        .oidc
        .complete_login(query.code, query.state)
        .await
        .map_err(authentication_error)?;
    let session = state
        .sessions
        .issue_session(actor)
        .await
        .map_err(authentication_error)?;
    let mut response = Redirect::to("/").into_response();
    for value in [
        format!(
            "{SESSION_COOKIE}={}; Path={}; Secure; HttpOnly; SameSite=Lax",
            session.session_id, state.cookie_path
        ),
        format!(
            "{CSRF_COOKIE}={}; Path={}; Secure; SameSite=Lax",
            session.csrf_token, state.cookie_path
        ),
    ] {
        response.headers_mut().append(
            header::SET_COOKIE,
            value.parse().map_err(|_| {
                problem(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "unavailable",
                    "authentication is unavailable",
                )
            })?,
        );
    }
    Ok(response)
}

async fn logout(State(state): State<V3ApiState>, headers: HeaderMap) -> V3Result<Response> {
    let _actor = authenticated_mutation_actor(&headers, &state).await?;
    let session_id =
        cookie_value(&headers, SESSION_COOKIE).expect("authenticated session cookie exists");
    state
        .sessions
        .revoke_session(session_id)
        .await
        .map_err(authentication_error)?;
    let mut response = StatusCode::NO_CONTENT.into_response();
    for value in [
        format!(
            "{SESSION_COOKIE}=; Path={}; Max-Age=0; Secure; HttpOnly; SameSite=Lax",
            state.cookie_path
        ),
        format!(
            "{CSRF_COOKIE}=; Path={}; Max-Age=0; Secure; SameSite=Lax",
            state.cookie_path
        ),
    ] {
        response.headers_mut().append(
            header::SET_COOKIE,
            value.parse().map_err(|_| {
                problem(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "unavailable",
                    "authentication is unavailable",
                )
            })?,
        );
    }
    Ok(response)
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({"status": "ok", "api_version": API_VERSION}))
}

fn mcp_endpoint(state: &V3ApiState) -> V3Result<&Arc<V3McpEndpoint>> {
    state
        .mcp
        .as_ref()
        .ok_or_else(|| problem(StatusCode::NOT_FOUND, "not_found", "MCP is not available"))
}

async fn mcp_resource_metadata(
    State(state): State<V3ApiState>,
) -> V3Result<Json<serde_json::Value>> {
    let endpoint = mcp_endpoint(&state)?;
    Ok(Json(
        serde_json::json!({"resource": endpoint.resource_uri, "authorization_servers": [endpoint.authorization_server_uri], "bearer_methods_supported": ["header"], "scopes_supported": ["notes:read", "notes:write", "notes:delete"]}),
    ))
}

async fn mcp_server_metadata(State(state): State<V3ApiState>) -> V3Result<Json<serde_json::Value>> {
    let endpoint = mcp_endpoint(&state)?;
    Ok(Json(
        serde_json::json!({"issuer": endpoint.authorization_server_uri, "authorization_endpoint": endpoint.authorization_endpoint_uri, "token_endpoint": endpoint.token_endpoint_uri, "response_types_supported": ["code"], "grant_types_supported": ["authorization_code", "refresh_token"], "code_challenge_methods_supported": ["S256"], "token_endpoint_auth_methods_supported": ["none"]}),
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

fn mcp_error(error: McpOAuthUseCaseError) -> (StatusCode, Json<Problem>) {
    match error {
        McpOAuthUseCaseError::Rejected => problem(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "OAuth request is invalid",
        ),
        McpOAuthUseCaseError::Unavailable => problem(
            StatusCode::SERVICE_UNAVAILABLE,
            "unavailable",
            "OAuth service is unavailable",
        ),
    }
}

fn authorize_fields(query: &McpAuthorizeQuery) -> V3Result<(Vec<String>, String)> {
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

async fn mcp_authorize(
    State(state): State<V3ApiState>,
    headers: HeaderMap,
    Query(query): Query<McpAuthorizeQuery>,
) -> V3Result<Html<String>> {
    let _actor = authenticated_actor(&headers, &state).await?;
    let _ = mcp_endpoint(&state)?;
    let (_scopes, _) = authorize_fields(&query)?;
    let csrf = cookie_value(&headers, CSRF_COOKIE).ok_or_else(|| {
        problem(
            StatusCode::FORBIDDEN,
            "csrf_required",
            "CSRF token is required",
        )
    })?;
    Ok(Html(format!(
        "<!doctype html><meta charset=\"utf-8\"><title>MCP authorization</title><main><h1>MCP authorization</h1><p>{}</p><form method=\"post\"><input type=\"hidden\" name=\"client_id\" value=\"{}\"><input type=\"hidden\" name=\"redirect_uri\" value=\"{}\"><input type=\"hidden\" name=\"resource\" value=\"{}\"><input type=\"hidden\" name=\"scope\" value=\"{}\"><input type=\"hidden\" name=\"code_challenge\" value=\"{}\"><input type=\"hidden\" name=\"state\" value=\"{}\"><input type=\"hidden\" name=\"csrf_token\" value=\"{}\"><button name=\"decision\" value=\"approve\">Allow</button><button name=\"decision\" value=\"deny\">Deny</button></form></main>",
        escape_html(&query.client_id),
        escape_html(&query.client_id),
        escape_html(&query.redirect_uri),
        escape_html(&query.resource),
        escape_html(&query.scope),
        escape_html(&query.code_challenge),
        escape_html(query.state.as_deref().unwrap_or_default()),
        escape_html(&csrf)
    )))
}

async fn mcp_authorize_submit(
    State(state): State<V3ApiState>,
    headers: HeaderMap,
    Form(form): Form<McpAuthorizeForm>,
) -> V3Result<Response> {
    let actor = authenticated_mutation_actor(&headers, &state).await?;
    let endpoint = mcp_endpoint(&state)?;
    if cookie_value(&headers, CSRF_COOKIE).as_deref() != Some(&form.csrf_token)
        || form.decision != "approve"
    {
        return Err(problem(
            StatusCode::FORBIDDEN,
            "access_denied",
            "authorization was denied",
        ));
    }
    let scopes = form
        .scope
        .split_ascii_whitespace()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let code = endpoint
        .oauth
        .authorize(
            actor,
            form.client_id,
            form.redirect_uri.clone(),
            form.resource,
            scopes,
            form.code_challenge,
        )
        .await
        .map_err(mcp_error)?;
    let mut url = url::Url::parse(&form.redirect_uri).map_err(|_| {
        problem(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "redirect URI is invalid",
        )
    })?;
    {
        let mut pairs = url.query_pairs_mut();
        pairs.append_pair("code", &code);
        if let Some(state) = form.state {
            pairs.append_pair("state", &state);
        }
    }
    Ok(Redirect::to(url.as_str()).into_response())
}

async fn mcp_token(
    State(state): State<V3ApiState>,
    Form(form): Form<McpTokenForm>,
) -> V3Result<Json<McpTokenResponse>> {
    let endpoint = mcp_endpoint(&state)?;
    let pair = match form.grant_type.as_str() {
        "authorization_code" => endpoint
            .oauth
            .exchange_authorization_code(
                form.code.ok_or_else(|| {
                    problem(
                        StatusCode::BAD_REQUEST,
                        "invalid_request",
                        "code is required",
                    )
                })?,
                form.client_id,
                form.redirect_uri.ok_or_else(|| {
                    problem(
                        StatusCode::BAD_REQUEST,
                        "invalid_request",
                        "redirect_uri is required",
                    )
                })?,
                form.resource,
                form.code_verifier.ok_or_else(|| {
                    problem(
                        StatusCode::BAD_REQUEST,
                        "invalid_request",
                        "code_verifier is required",
                    )
                })?,
            )
            .await
            .map_err(mcp_error)?,
        "refresh_token" => endpoint
            .oauth
            .refresh_access_token(
                form.refresh_token.ok_or_else(|| {
                    problem(
                        StatusCode::BAD_REQUEST,
                        "invalid_request",
                        "refresh_token is required",
                    )
                })?,
                form.client_id,
                form.resource,
            )
            .await
            .map_err(mcp_error)?,
        _ => {
            return Err(problem(
                StatusCode::BAD_REQUEST,
                "unsupported_grant_type",
                "OAuth grant type is unsupported",
            ));
        }
    };
    Ok(Json(McpTokenResponse {
        access_token: pair.access_token,
        refresh_token: pair.refresh_token,
        token_type: "Bearer",
        expires_in: pair.access_expires_in_seconds,
        scope: pair.scope,
    }))
}

async fn session(
    State(state): State<V3ApiState>,
    headers: HeaderMap,
) -> V3Result<Json<SessionResponse>> {
    let actor = authenticated_actor(&headers, &state).await?;
    Ok(Json(SessionResponse {
        issuer: actor.issuer,
        subject: actor.subject,
        is_administrator: actor.is_administrator,
    }))
}

async fn list_notes(
    State(state): State<V3ApiState>,
    headers: HeaderMap,
) -> V3Result<Json<Vec<NoteResponse>>> {
    let actor = authenticated_actor(&headers, &state).await?;
    let notes = state
        .notes
        .list_visible_notes(actor)
        .await
        .map_err(note_error)?;
    Ok(Json(notes.into_iter().map(NoteResponse::from).collect()))
}

async fn read_note(
    State(state): State<V3ApiState>,
    Path(note_id): Path<String>,
    headers: HeaderMap,
) -> V3Result<Json<NoteResponse>> {
    let actor = authenticated_actor(&headers, &state).await?;
    let note = state
        .notes
        .read_note(actor, parse_note_id(&note_id)?)
        .await
        .map_err(note_error)?;
    Ok(Json(note.into()))
}

async fn create_note(
    State(state): State<V3ApiState>,
    headers: HeaderMap,
    Json(input): Json<NoteInput>,
) -> V3Result<(StatusCode, Json<NoteResponse>)> {
    let actor = authenticated_mutation_actor(&headers, &state).await?;
    let note = state
        .notes
        .create_note(
            actor,
            CanonicalNoteDraft {
                title: input.title,
                body: input.body,
                tags: input.tags,
            },
        )
        .await
        .map_err(note_error)?;
    Ok((StatusCode::CREATED, Json(note.into())))
}

async fn update_note(
    State(state): State<V3ApiState>,
    Path(note_id): Path<String>,
    headers: HeaderMap,
    Json(input): Json<NoteUpdateInput>,
) -> V3Result<Json<NoteResponse>> {
    let actor = authenticated_mutation_actor(&headers, &state).await?;
    let note = state
        .notes
        .update_note(
            actor,
            parse_note_id(&note_id)?,
            CanonicalNoteDraft {
                title: input.title,
                body: input.body,
                tags: input.tags,
            },
            input.expected_revision,
        )
        .await
        .map_err(note_error)?;
    Ok(Json(note.into()))
}

async fn delete_note(
    State(state): State<V3ApiState>,
    Path(note_id): Path<String>,
    headers: HeaderMap,
    Json(input): Json<DeleteInput>,
) -> V3Result<Json<NoteResponse>> {
    let actor = authenticated_mutation_actor(&headers, &state).await?;
    let note = state
        .notes
        .soft_delete_note(actor, parse_note_id(&note_id)?, input.expected_revision)
        .await
        .map_err(note_error)?;
    Ok(Json(note.into()))
}

async fn restore_note(
    State(state): State<V3ApiState>,
    Path(note_id): Path<String>,
    headers: HeaderMap,
    Json(input): Json<DeleteInput>,
) -> V3Result<Json<NoteResponse>> {
    let actor = authenticated_mutation_actor(&headers, &state).await?;
    let note = state
        .notes
        .restore_note(actor, parse_note_id(&note_id)?, input.expected_revision)
        .await
        .map_err(note_error)?;
    Ok(Json(note.into()))
}

async fn export_note(
    State(state): State<V3ApiState>,
    Path(note_id): Path<String>,
    headers: HeaderMap,
) -> V3Result<Response> {
    let actor = authenticated_actor(&headers, &state).await?;
    let note = state
        .notes
        .read_note(actor, parse_note_id(&note_id)?)
        .await
        .map_err(note_error)?;
    let source = marginalis_asciidoc::export_canonical_note(&note).map_err(|_| {
        problem(
            StatusCode::SERVICE_UNAVAILABLE,
            "unavailable",
            "note export is unavailable",
        )
    })?;
    Ok((
        [(header::CONTENT_TYPE, "text/asciidoc; charset=utf-8")],
        source,
    )
        .into_response())
}

async fn home(State(state): State<V3ApiState>, headers: HeaderMap) -> V3Result<Html<String>> {
    let actor = authenticated_actor(&headers, &state).await?;
    let notes = state
        .notes
        .list_visible_notes(actor)
        .await
        .map_err(note_error)?;
    let list = notes
        .into_iter()
        .map(|note| {
            format!(
                "<li><a href=\"/notes/{}\">{}</a></li>",
                note.note_id,
                escape_html(&note.title)
            )
        })
        .collect::<String>();
    Ok(Html(format!(
        "<!doctype html><meta charset=\"utf-8\"><title>Marginalis</title><main><h1>Marginalis</h1><p>閲覧できるノート</p><ul>{list}</ul></main>"
    )))
}

async fn view_note(
    State(state): State<V3ApiState>,
    Path(note_id): Path<String>,
    headers: HeaderMap,
) -> V3Result<Html<String>> {
    let actor = authenticated_actor(&headers, &state).await?;
    let note = state
        .notes
        .read_note(actor, parse_note_id(&note_id)?)
        .await
        .map_err(note_error)?;
    let body = marginalis_asciidoc::render_canonical_note_html(&note).map_err(|_| {
        problem(
            StatusCode::UNPROCESSABLE_ENTITY,
            "render_failed",
            "note cannot be rendered safely",
        )
    })?;
    Ok(Html(format!(
        "<!doctype html><meta charset=\"utf-8\"><title>{}</title><main><p><a href=\"/\">一覧</a></p>{}</main>",
        escape_html(&note.title),
        body
    )))
}

async fn authenticated_actor(headers: &HeaderMap, state: &V3ApiState) -> V3Result<CanonicalActor> {
    let session_id = cookie_value(headers, SESSION_COOKIE).ok_or_else(|| {
        problem(
            StatusCode::UNAUTHORIZED,
            "authentication_required",
            "authentication is required",
        )
    })?;
    state
        .sessions
        .authenticate_session(session_id)
        .await
        .map_err(authentication_error)?
        .map(|session| session.actor)
        .ok_or_else(|| {
            problem(
                StatusCode::UNAUTHORIZED,
                "authentication_required",
                "authentication is required",
            )
        })
}

async fn authenticated_mutation_actor(
    headers: &HeaderMap,
    state: &V3ApiState,
) -> V3Result<CanonicalActor> {
    let actor = authenticated_actor(headers, state).await?;
    if headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
        != Some(state.browser_origin.as_str())
    {
        return Err(problem(
            StatusCode::FORBIDDEN,
            "same_origin_required",
            "same-origin request is required",
        ));
    }
    if let Some(site) = headers
        .get("sec-fetch-site")
        .and_then(|value| value.to_str().ok())
    {
        if !matches!(site, "same-origin" | "none") {
            return Err(problem(
                StatusCode::FORBIDDEN,
                "same_origin_required",
                "same-origin request is required",
            ));
        }
    }
    let session_id =
        cookie_value(headers, SESSION_COOKIE).expect("authenticated session cookie exists");
    let csrf_cookie = cookie_value(headers, CSRF_COOKIE).ok_or_else(|| {
        problem(
            StatusCode::FORBIDDEN,
            "csrf_required",
            "CSRF token is required",
        )
    })?;
    let csrf_header = headers
        .get("x-csrf-token")
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| {
            problem(
                StatusCode::FORBIDDEN,
                "csrf_required",
                "CSRF token is required",
            )
        })?;
    if csrf_cookie != csrf_header
        || !state
            .sessions
            .verify_csrf(session_id, csrf_header.into())
            .await
            .map_err(authentication_error)?
    {
        return Err(problem(
            StatusCode::FORBIDDEN,
            "csrf_invalid",
            "CSRF token is invalid",
        ));
    }
    Ok(actor)
}

fn parse_note_id(value: &str) -> V3Result<NoteId> {
    EntityId::from_str(value)
        .map(NoteId::new)
        .map_err(|_| problem(StatusCode::NOT_FOUND, "not_found", "note is not available"))
}

fn cookie_value(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .find_map(|part| {
            let (key, value) = part.trim().split_once('=')?;
            (key == name).then(|| value.to_owned())
        })
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use axum::{body::Body, http::Request};
    use marginalis_application::{
        AuthenticationUseCaseError, McpOAuthUseCaseError, NoteUseCaseError, V3McpTokenPair,
    };
    use marginalis_domain::{
        CanonicalAuthenticatedSession, CanonicalMcpAuthenticatedActor, CanonicalWebSession,
        McpOAuthClient,
    };
    use tower::ServiceExt;

    struct Notes;

    #[async_trait]
    impl V3NoteUseCases for Notes {
        async fn list_visible_notes(
            &self,
            _actor: CanonicalActor,
        ) -> Result<Vec<CanonicalNote>, NoteUseCaseError> {
            Ok(Vec::new())
        }

        async fn read_note(
            &self,
            _actor: CanonicalActor,
            _note_id: NoteId,
        ) -> Result<CanonicalNote, NoteUseCaseError> {
            Err(NoteUseCaseError::NotFound)
        }

        async fn create_note(
            &self,
            _actor: CanonicalActor,
            _draft: CanonicalNoteDraft,
        ) -> Result<CanonicalNote, NoteUseCaseError> {
            Err(NoteUseCaseError::Unavailable)
        }

        async fn update_note(
            &self,
            _actor: CanonicalActor,
            _note_id: NoteId,
            _draft: CanonicalNoteDraft,
            _expected_revision: i64,
        ) -> Result<CanonicalNote, NoteUseCaseError> {
            Err(NoteUseCaseError::Unavailable)
        }

        async fn soft_delete_note(
            &self,
            _actor: CanonicalActor,
            _note_id: NoteId,
            _expected_revision: i64,
        ) -> Result<CanonicalNote, NoteUseCaseError> {
            Err(NoteUseCaseError::Unavailable)
        }

        async fn restore_note(
            &self,
            _actor: CanonicalActor,
            _note_id: NoteId,
            _expected_revision: i64,
        ) -> Result<CanonicalNote, NoteUseCaseError> {
            Err(NoteUseCaseError::Unavailable)
        }
    }

    struct Sessions;

    #[async_trait]
    impl V3WebSessionUseCases for Sessions {
        async fn authenticate_session(
            &self,
            _session_id: String,
        ) -> Result<Option<CanonicalAuthenticatedSession>, AuthenticationUseCaseError> {
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
            _actor: CanonicalActor,
        ) -> Result<CanonicalWebSession, AuthenticationUseCaseError> {
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
    impl V3OidcAuthenticationUseCases for Oidc {
        async fn begin_login(&self) -> Result<String, AuthenticationUseCaseError> {
            Err(AuthenticationUseCaseError::Unavailable)
        }

        async fn complete_login(
            &self,
            _code: String,
            _state: String,
        ) -> Result<CanonicalActor, AuthenticationUseCaseError> {
            Err(AuthenticationUseCaseError::Unavailable)
        }
    }

    struct Mcp;
    #[async_trait]
    impl V3McpOAuthUseCases for Mcp {
        async fn register_client(
            &self,
            _client: McpOAuthClient,
        ) -> Result<(), McpOAuthUseCaseError> {
            Ok(())
        }
        async fn authorize(
            &self,
            _actor: CanonicalActor,
            _client_id: String,
            _redirect_uri: String,
            _resource_uri: String,
            _scopes: Vec<String>,
            _code_challenge: String,
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
        ) -> Result<V3McpTokenPair, McpOAuthUseCaseError> {
            Err(McpOAuthUseCaseError::Rejected)
        }
        async fn refresh_access_token(
            &self,
            _refresh_token: String,
            _client_id: String,
            _resource_uri: String,
        ) -> Result<V3McpTokenPair, McpOAuthUseCaseError> {
            Err(McpOAuthUseCaseError::Rejected)
        }
        async fn authenticate(
            &self,
            _token: String,
            _resource_uri: String,
            _scope: String,
        ) -> Result<Option<CanonicalMcpAuthenticatedActor>, McpOAuthUseCaseError> {
            Ok(None)
        }
        async fn revoke(
            &self,
            _actor: CanonicalActor,
            _client_id: String,
        ) -> Result<(), McpOAuthUseCaseError> {
            Ok(())
        }
    }

    fn app() -> Router {
        router(V3ApiState::new(
            Arc::new(Notes),
            Arc::new(Sessions),
            Arc::new(Oidc),
            "/".into(),
            "https://example.test".into(),
        ))
    }

    fn mcp_app() -> Router {
        router(
            V3ApiState::new(
                Arc::new(Notes),
                Arc::new(Sessions),
                Arc::new(Oidc),
                "/".into(),
                "https://example.test".into(),
            )
            .with_mcp(V3McpEndpoint {
                oauth: Arc::new(Mcp),
                resource_uri: "https://example.test/mcp".into(),
                metadata_uri: "https://example.test/.well-known/oauth-protected-resource/mcp"
                    .into(),
                authorization_server_uri: "https://example.test".into(),
                authorization_endpoint_uri: "https://example.test/oauth/authorize".into(),
                token_endpoint_uri: "https://example.test/oauth/token".into(),
            }),
        )
    }

    #[tokio::test]
    async fn v3_health_is_public_but_notes_require_a_v3_session() {
        let health = app()
            .oneshot(
                Request::get("/api/v2/health")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(health.status(), StatusCode::OK);
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
    async fn v3_openapi_is_served_from_the_embedded_contract() {
        let response = app()
            .oneshot(
                Request::get("/api/v2/openapi.json")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            OPENAPI_DOCUMENT,
            include_str!("../../../docs/openapi-v3.json")
        );
    }

    #[tokio::test]
    async fn v3_mcp_metadata_is_available_when_enabled() {
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
}
