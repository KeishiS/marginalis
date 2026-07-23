//! MarginalisのWeb APIにおけるHTTP境界。
//!
//! 認証、Web UIおよびMCPはこのcrateのHTTP adapterとして追加する。ノートの検証、ACLおよび
//! 永続化の業務判断は`marginalis-application`のユースケースへ委譲する。

use std::{
    collections::HashMap,
    str::FromStr,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use axum::{
    Json, Router,
    extract::{Form, Path, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{Html, IntoResponse, Redirect, Response},
    routing::{delete, get, post, put},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use marginalis_application::{
    AuthenticationUseCaseError, McpAuthorizationRequest, McpOAuthUseCaseError, McpOAuthUseCases,
    NoteUseCaseError, NoteUseCases, WebAuthenticationUseCases, WebSession,
};
pub use marginalis_auth_oidc::{
    OidcAuthentication, OidcCallbackError, OidcCallbackRejection, OidcConfiguration,
    OidcConfigurationError, OidcDiscoveryError, OidcLoginStartError,
};
use marginalis_domain::{
    Actor, EntityId, NoteId, NotePage, NotePermission, NoteSummary, OidcLoginResult, OidcUser,
    SourceRevision, UserId,
};
use marginalis_mcp::{JsonRpcRequest, McpAuthenticationError, McpAuthenticator, McpTools};
use serde::{Deserialize, Serialize};
use tower_http::trace::{DefaultOnResponse, TraceLayer};
use tracing::Level;
use tracing::info_span;
use url::Url;

/// 公開REST APIの現在のバージョン。
pub const API_VERSION: &str = "v1";

/// HTTPハンドラーが利用する共有状態。
#[derive(Clone)]
pub struct ApiState {
    pub notes: Arc<dyn NoteUseCases>,
    pub authentication: Arc<dyn WebAuthenticationUseCases>,
    pub mcp: Option<Arc<McpEndpoint>>,
}

pub struct McpEndpoint {
    pub tools: McpTools,
    pub authenticator: Arc<dyn McpAuthenticator>,
    pub oauth: Arc<dyn McpOAuthUseCases>,
    pub resource_uri: String,
    pub metadata_uri: String,
    pub authorization_server_uri: String,
    pub authorization_endpoint_uri: String,
    pub token_endpoint_uri: String,
    pub allowed_origin: String,
    pub rate_limiter: McpRateLimiter,
}

/// MCP tool呼出しの利用者単位固定window rate limiter。
///
/// tokenの内容は保持せず、server再起動時にはwindowを破棄する。永続的な利用量制御は運用監査基盤の
/// 導入時に別途追加する。
pub struct McpRateLimiter {
    requests_per_minute: u32,
    windows: Mutex<HashMap<String, (Instant, u32)>>,
}

impl McpRateLimiter {
    pub fn new(requests_per_minute: u32) -> Self {
        Self {
            requests_per_minute,
            windows: Mutex::new(HashMap::new()),
        }
    }

    fn allow(&self, actor: Actor) -> bool {
        let Ok(mut windows) = self.windows.lock() else {
            return false;
        };
        let now = Instant::now();
        let window = windows.entry(actor.user_id.to_string()).or_insert((now, 0));
        if now.duration_since(window.0) >= Duration::from_secs(60) {
            *window = (now, 0);
        }
        if window.1 >= self.requests_per_minute {
            return false;
        }
        window.1 += 1;
        true
    }
}

impl ApiState {
    pub fn new(
        notes: Arc<dyn NoteUseCases>,
        authentication: Arc<dyn WebAuthenticationUseCases>,
    ) -> Self {
        Self {
            notes,
            authentication,
            mcp: None,
        }
    }

    pub fn with_mcp(mut self, mcp: McpEndpoint) -> Self {
        self.mcp = Some(Arc::new(mcp));
        self
    }

    #[cfg(test)]
    fn with_test_adapters(
        database: marginalis_sqlite::SqliteDatabase,
        notes: Arc<dyn NoteUseCases>,
    ) -> Self {
        Self::new(
            notes,
            Arc::new(marginalis_server::ServerWebAuthenticationUseCases::new(
                database,
            )),
        )
    }
}

/// 認証アダプタだけが生成する、リクエストに結び付いた利用者文脈。
///
/// OAuth、Cookie sessionおよび将来のMCP tokenは異なるが、ACL判定に渡す値はこの型へ統一する。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthenticatedActor {
    pub actor: Actor,
}

/// HTTP APIの安定したエラーコード。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApiErrorCode {
    AuthenticationRequired,
    Forbidden,
    NotFound,
    ValidationFailed,
    Conflict,
    Internal,
}

impl ApiErrorCode {
    const fn as_str(self) -> &'static str {
        match self {
            Self::AuthenticationRequired => "authentication-required",
            Self::Forbidden => "forbidden",
            Self::NotFound => "not-found",
            Self::ValidationFailed => "validation-failed",
            Self::Conflict => "conflict",
            Self::Internal => "internal",
        }
    }

    const fn status(self) -> StatusCode {
        match self {
            Self::AuthenticationRequired => StatusCode::UNAUTHORIZED,
            Self::Forbidden => StatusCode::FORBIDDEN,
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::ValidationFailed => StatusCode::UNPROCESSABLE_ENTITY,
            Self::Conflict => StatusCode::CONFLICT,
            Self::Internal => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

/// JSON APIの失敗応答。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApiError {
    pub code: ApiErrorCode,
    /// クライアントに安全に公開できる説明。DB、ACL、token等を含めない。
    pub message: &'static str,
}

impl ApiError {
    pub const fn new(code: ApiErrorCode, message: &'static str) -> Self {
        Self { code, message }
    }
}

fn note_error(error: NoteUseCaseError, unavailable_message: &'static str) -> ApiError {
    match error {
        NoteUseCaseError::NotFound => {
            ApiError::new(ApiErrorCode::NotFound, "note is not available")
        }
        NoteUseCaseError::Forbidden => {
            ApiError::new(ApiErrorCode::Forbidden, "note operation is not permitted")
        }
        NoteUseCaseError::Conflict => {
            ApiError::new(ApiErrorCode::Conflict, "note operation conflicts")
        }
        NoteUseCaseError::Validation => {
            ApiError::new(ApiErrorCode::ValidationFailed, "note source is invalid")
        }
        NoteUseCaseError::Unavailable => ApiError::new(ApiErrorCode::Internal, unavailable_message),
    }
}

fn authentication_error(error: AuthenticationUseCaseError) -> ApiError {
    match error {
        AuthenticationUseCaseError::Rejected => ApiError::new(
            ApiErrorCode::AuthenticationRequired,
            "authentication failed",
        ),
        AuthenticationUseCaseError::NotFound => {
            ApiError::new(ApiErrorCode::NotFound, "user is not available")
        }
        AuthenticationUseCaseError::Unavailable => {
            ApiError::new(ApiErrorCode::Internal, "authentication is unavailable")
        }
    }
}

#[derive(Serialize)]
struct ApiErrorBody {
    code: &'static str,
    message: &'static str,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.code.status(),
            Json(ApiErrorBody {
                code: self.code.as_str(),
                message: self.message,
            }),
        )
            .into_response()
    }
}

/// Web UI、REST APIおよび将来のMCP endpointを収容するルーター。
pub fn router(state: ApiState) -> Router {
    Router::new()
        .route("/", get(landing))
        .route("/api/v1/health", get(health))
        .route("/api/v1/session", get(current_session))
        .route("/api/v1/notes/{note_id}/source", get(note_source))
        .route("/api/v1/notes/{note_id}", get(note_metadata))
        .route("/api/v1/notes/{note_id}/source", put(update_note_source))
        .route("/api/v1/notes/{note_id}", delete(delete_note))
        .route("/api/v1/notes", get(list_notes).post(create_note))
        .route("/api/v1/search", get(search_notes))
        .route("/mcp", get(mcp_get).post(mcp_post))
        .route(
            "/.well-known/oauth-protected-resource/mcp",
            get(mcp_protected_resource_metadata),
        )
        .route(
            "/.well-known/oauth-authorization-server",
            get(mcp_authorization_server_metadata),
        )
        .route(
            "/oauth/authorize",
            get(mcp_authorize).post(mcp_authorize_submit),
        )
        .route("/oauth/token", post(mcp_token))
        .route(
            "/api/v1/notes/{note_id}/acl/{user_id}",
            put(update_note_acl),
        )
        .route("/auth/oidc/login", get(begin_oidc_login))
        .route("/auth/oidc/callback", get(complete_oidc_login))
        .route("/auth/root/login", post(root_login))
        .route("/auth/logout", post(logout))
        .route("/api/v1/admin/users/pending", get(list_pending_users))
        .route(
            "/api/v1/admin/users/{user_id}/activate",
            put(activate_pending_user),
        )
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(|request: &axum::http::Request<_>| {
                    info_span!(
                        "http_request",
                        method = %request.method(),
                        path = request.uri().path(),
                    )
                })
                .on_response(DefaultOnResponse::new().level(Level::INFO)),
        )
        .with_state(state)
}

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
    api_version: &'static str,
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        api_version: API_VERSION,
    })
}

async fn mcp_get() -> StatusCode {
    // 初期実装はserver-to-client notification streamを持たない。
    StatusCode::METHOD_NOT_ALLOWED
}

async fn mcp_protected_resource_metadata(
    State(state): State<ApiState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let endpoint = state.mcp.as_ref().ok_or(ApiError::new(
        ApiErrorCode::NotFound,
        "MCP is not available",
    ))?;
    Ok(Json(serde_json::json!({
        "resource": endpoint.resource_uri,
        "authorization_servers": [endpoint.authorization_server_uri],
        "bearer_methods_supported": ["header"],
        "scopes_supported": ["notes:read", "notes:write", "notes:delete"]
    })))
}

async fn mcp_authorization_server_metadata(
    State(state): State<ApiState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let endpoint = state.mcp.as_ref().ok_or(ApiError::new(
        ApiErrorCode::NotFound,
        "MCP is not available",
    ))?;
    Ok(Json(serde_json::json!({
        "issuer": endpoint.authorization_server_uri,
        "authorization_endpoint": endpoint.authorization_endpoint_uri,
        "token_endpoint": endpoint.token_endpoint_uri,
        "response_types_supported": ["code"],
        "grant_types_supported": ["authorization_code", "refresh_token"],
        "code_challenge_methods_supported": ["S256"],
        "scopes_supported": ["notes:read", "notes:write", "notes:delete"],
        "token_endpoint_auth_methods_supported": ["none"]
    })))
}

async fn mcp_post(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(request): Json<JsonRpcRequest>,
) -> Result<Response, ApiError> {
    let endpoint = state.mcp.as_ref().ok_or(ApiError::new(
        ApiErrorCode::NotFound,
        "MCP is not available",
    ))?;
    if let Some(origin) = headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
        && origin != endpoint.allowed_origin
    {
        return Err(ApiError::new(
            ApiErrorCode::Forbidden,
            "MCP origin is not allowed",
        ));
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
    let required_scope = endpoint.tools.required_scope(&request);
    let actor = match endpoint
        .authenticator
        .authenticate(token, required_scope)
        .await
    {
        Ok(actor) => actor,
        Err(
            McpAuthenticationError::MissingOrInvalid | McpAuthenticationError::InsufficientScope,
        ) => {
            return Ok(mcp_unauthorized(endpoint));
        }
        Err(McpAuthenticationError::Unavailable) => {
            return Err(ApiError::new(
                ApiErrorCode::Internal,
                "MCP authentication is unavailable",
            ));
        }
    };
    if !endpoint.rate_limiter.allow(actor) {
        let mut response = StatusCode::TOO_MANY_REQUESTS.into_response();
        response
            .headers_mut()
            .insert(header::RETRY_AFTER, HeaderValue::from_static("60"));
        return Ok(response);
    }
    match endpoint.tools.handle(actor, request).await {
        Some(response) => Ok(Json(response).into_response()),
        None => Ok(StatusCode::ACCEPTED.into_response()),
    }
}

fn mcp_unauthorized(endpoint: &McpEndpoint) -> Response {
    let mut response = StatusCode::UNAUTHORIZED.into_response();
    let value = format!("Bearer resource_metadata=\"{}\"", endpoint.metadata_uri);
    if let Ok(value) = HeaderValue::from_str(&value) {
        response
            .headers_mut()
            .insert(header::WWW_AUTHENTICATE, value);
    }
    response
}

#[derive(Clone, Deserialize, Serialize)]
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

fn oauth_request(query: &McpAuthorizeQuery) -> Result<McpAuthorizationRequest, ApiError> {
    if query.response_type != "code" || query.code_challenge_method != "S256" {
        return Err(ApiError::new(
            ApiErrorCode::ValidationFailed,
            "OAuth authorization request is invalid",
        ));
    }
    Ok(McpAuthorizationRequest {
        client_id: query.client_id.clone(),
        redirect_uri: query.redirect_uri.clone(),
        resource_uri: query.resource.clone(),
        scopes: query
            .scope
            .split_ascii_whitespace()
            .map(str::to_owned)
            .collect(),
        code_challenge: query.code_challenge.clone(),
    })
}

fn oauth_error(error: McpOAuthUseCaseError) -> ApiError {
    match error {
        McpOAuthUseCaseError::Rejected => ApiError::new(
            ApiErrorCode::ValidationFailed,
            "OAuth authorization request is invalid",
        ),
        McpOAuthUseCaseError::Unavailable => {
            ApiError::new(ApiErrorCode::Internal, "OAuth service is unavailable")
        }
    }
}

async fn mcp_authorize(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Query(query): Query<McpAuthorizeQuery>,
) -> Result<Response, ApiError> {
    let endpoint = state.mcp.as_ref().ok_or(ApiError::new(
        ApiErrorCode::NotFound,
        "MCP is not available",
    ))?;
    let request = oauth_request(&query)?;
    let client = endpoint
        .oauth
        .validate_authorization_request(request)
        .await
        .map_err(oauth_error)?;
    let actor = match authenticated_actor(&headers, &state).await {
        Ok(actor) => actor,
        Err(error) if error.code == ApiErrorCode::AuthenticationRequired => {
            return oidc_login_with_return_to(&state, &query).await;
        }
        Err(error) => return Err(error),
    };
    if actor.is_root {
        return Err(ApiError::new(
            ApiErrorCode::Forbidden,
            "root sessions cannot authorize MCP clients",
        ));
    }
    let csrf = cookie_value(&headers, "marginalis_csrf").ok_or(ApiError::new(
        ApiErrorCode::Forbidden,
        "CSRF token is required",
    ))?;
    let body = format!(
        "<!doctype html><meta charset=\"utf-8\"><title>Marginalis authorization</title><h1>Authorize {}</h1><p>This client requests: {}</p><form method=\"post\" action=\"authorize\"><input type=\"hidden\" name=\"client_id\" value=\"{}\"><input type=\"hidden\" name=\"redirect_uri\" value=\"{}\"><input type=\"hidden\" name=\"resource\" value=\"{}\"><input type=\"hidden\" name=\"scope\" value=\"{}\"><input type=\"hidden\" name=\"code_challenge\" value=\"{}\"><input type=\"hidden\" name=\"state\" value=\"{}\"><input type=\"hidden\" name=\"csrf_token\" value=\"{}\"><button name=\"decision\" value=\"approve\" type=\"submit\">Allow</button><button name=\"decision\" value=\"deny\" type=\"submit\">Deny</button></form>",
        escape_html(&client.display_name),
        escape_html(&query.scope),
        escape_html(&query.client_id),
        escape_html(&query.redirect_uri),
        escape_html(&query.resource),
        escape_html(&query.scope),
        escape_html(&query.code_challenge),
        escape_html(query.state.as_deref().unwrap_or_default()),
        escape_html(&csrf),
    );
    Ok(Html(body).into_response())
}

async fn mcp_authorize_submit(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Form(form): Form<McpAuthorizeForm>,
) -> Result<Response, ApiError> {
    let endpoint = state.mcp.as_ref().ok_or(ApiError::new(
        ApiErrorCode::NotFound,
        "MCP is not available",
    ))?;
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
    let actor = authenticated_actor(&headers, &state).await?;
    require_csrf_token(&headers, &state, form.csrf_token).await?;
    endpoint
        .oauth
        .validate_authorization_request(request.clone())
        .await
        .map_err(oauth_error)?;
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
        .map_err(oauth_error)?;
    oauth_redirect(
        &request.redirect_uri,
        form.state.as_deref(),
        Some(&code),
        None,
    )
}

async fn mcp_token(
    State(state): State<ApiState>,
    Form(form): Form<McpTokenForm>,
) -> Result<Json<McpTokenResponse>, ApiError> {
    let endpoint = state.mcp.as_ref().ok_or(ApiError::new(
        ApiErrorCode::NotFound,
        "MCP is not available",
    ))?;
    let tokens = match form.grant_type.as_str() {
        "authorization_code" => endpoint
            .oauth
            .exchange_authorization_code(
                required_token_field(form.code, "code")?,
                form.client_id,
                required_token_field(form.redirect_uri, "redirect_uri")?,
                form.resource,
                required_token_field(form.code_verifier, "code_verifier")?,
            )
            .await
            .map_err(oauth_error)?,
        "refresh_token" => endpoint
            .oauth
            .refresh_access_token(
                required_token_field(form.refresh_token, "refresh_token")?,
                form.client_id,
                form.resource,
            )
            .await
            .map_err(oauth_error)?,
        _ => {
            return Err(ApiError::new(
                ApiErrorCode::ValidationFailed,
                "OAuth grant type is not supported",
            ));
        }
    };
    Ok(Json(McpTokenResponse {
        access_token: tokens.access_token,
        refresh_token: tokens.refresh_token,
        token_type: "Bearer",
        expires_in: tokens.access_expires_in_seconds,
        scope: tokens.scope,
    }))
}

fn required_token_field(value: Option<String>, name: &'static str) -> Result<String, ApiError> {
    value.filter(|value| !value.is_empty()).ok_or(ApiError::new(
        ApiErrorCode::ValidationFailed,
        match name {
            "code" => "OAuth code is required",
            "redirect_uri" => "OAuth redirect URI is required",
            "code_verifier" => "OAuth code verifier is required",
            "refresh_token" => "OAuth refresh token is required",
            _ => "OAuth request is invalid",
        },
    ))
}

async fn oidc_login_with_return_to(
    state: &ApiState,
    query: &McpAuthorizeQuery,
) -> Result<Response, ApiError> {
    let destination = state
        .authentication
        .begin_oidc_login()
        .await
        .map_err(authentication_error)?;
    let return_to = oauth_authorize_return_to(query);
    let cookie_path = state.authentication.cookie_path();
    let cookie = format!(
        "marginalis_oauth_return_to={}; Path={cookie_path}; Max-Age=300; Secure; HttpOnly; SameSite=Lax",
        url::form_urlencoded::byte_serialize(return_to.as_bytes()).collect::<String>(),
    );
    let mut response = Redirect::temporary(&destination).into_response();
    response.headers_mut().insert(
        header::SET_COOKIE,
        HeaderValue::from_str(&cookie)
            .map_err(|_| ApiError::new(ApiErrorCode::Internal, "authentication is unavailable"))?,
    );
    Ok(response)
}

fn oauth_authorize_return_to(query: &McpAuthorizeQuery) -> String {
    let mut pairs = url::form_urlencoded::Serializer::new(String::new());
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
    format!("/oauth/authorize?{}", pairs.finish())
}

fn oauth_redirect(
    redirect_uri: &str,
    state: Option<&str>,
    code: Option<&str>,
    error: Option<&str>,
) -> Result<Response, ApiError> {
    let mut url = Url::parse(redirect_uri).map_err(|_| {
        ApiError::new(
            ApiErrorCode::ValidationFailed,
            "OAuth redirect URI is invalid",
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

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// Web UI公開前のBase URL到達先。OIDC callback後のredirect先としても用いる。
async fn landing() -> Json<HealthResponse> {
    health().await
}

#[derive(Serialize)]
struct CurrentSessionResponse {
    user_id: String,
    is_root: bool,
}

async fn current_session(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Result<Json<CurrentSessionResponse>, ApiError> {
    let actor = authenticated_actor(&headers, &state).await?;
    Ok(Json(CurrentSessionResponse {
        user_id: actor.user_id.to_string(),
        is_root: actor.is_root,
    }))
}

#[derive(Deserialize)]
struct NoteListQuery {
    limit: Option<u32>,
    cursor: Option<String>,
}

#[derive(Deserialize)]
struct NoteSearchQuery {
    q: String,
    limit: Option<u32>,
    cursor: Option<String>,
}

#[derive(Serialize)]
struct NoteSummaryResponse {
    note_id: String,
    title: String,
}

#[derive(Serialize)]
struct NoteMetadataResponse {
    note_id: String,
    title: String,
    revision: String,
}

#[derive(Serialize)]
struct NotePageResponse {
    notes: Vec<NoteSummaryResponse>,
    next_cursor: Option<String>,
}

async fn list_notes(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Query(query): Query<NoteListQuery>,
) -> Result<Json<NotePageResponse>, ApiError> {
    let actor = authenticated_actor(&headers, &state).await?;
    let page = state
        .notes
        .list_notes(
            actor,
            cursor_offset(query.cursor)?,
            bounded_limit(query.limit),
        )
        .await
        .map_err(|error| note_error(error, "note listing is unavailable"))?;
    Ok(Json(note_page_response(page)))
}

async fn search_notes(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Query(query): Query<NoteSearchQuery>,
) -> Result<Json<NotePageResponse>, ApiError> {
    let actor = authenticated_actor(&headers, &state).await?;
    let page = state
        .notes
        .search_notes(
            actor,
            query.q,
            cursor_offset(query.cursor)?,
            bounded_limit(query.limit),
        )
        .await
        .map_err(|error| note_error(error, "note search is unavailable"))?;
    Ok(Json(note_page_response(page)))
}

fn bounded_limit(value: Option<u32>) -> u32 {
    match value.unwrap_or(50) {
        0 => 1,
        value if value > 100 => 100,
        value => value,
    }
}

fn cursor_offset(cursor: Option<String>) -> Result<u64, ApiError> {
    let Some(cursor) = cursor else {
        return Ok(0);
    };
    let bytes = URL_SAFE_NO_PAD
        .decode(cursor)
        .map_err(|_| ApiError::new(ApiErrorCode::ValidationFailed, "cursor is invalid"))?;
    let bytes: [u8; 8] = bytes
        .try_into()
        .map_err(|_| ApiError::new(ApiErrorCode::ValidationFailed, "cursor is invalid"))?;
    Ok(u64::from_be_bytes(bytes))
}

fn next_cursor(offset: Option<u64>) -> Option<String> {
    offset.map(|offset| URL_SAFE_NO_PAD.encode(offset.to_be_bytes()))
}

fn etag(revision: SourceRevision) -> String {
    format!("\"{}\"", revision.to_hex())
}

fn required_if_match(headers: &HeaderMap) -> Result<SourceRevision, ApiError> {
    let value = headers
        .get(header::IF_MATCH)
        .and_then(|value| value.to_str().ok())
        .ok_or(ApiError::new(
            ApiErrorCode::Conflict,
            "If-Match is required",
        ))?;
    let value = value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .ok_or(ApiError::new(
            ApiErrorCode::ValidationFailed,
            "If-Match is invalid",
        ))?;
    SourceRevision::from_hex(value).ok_or(ApiError::new(
        ApiErrorCode::ValidationFailed,
        "If-Match is invalid",
    ))
}

fn note_summary_response(note: NoteSummary) -> NoteSummaryResponse {
    NoteSummaryResponse {
        note_id: note.note_id.to_string(),
        title: note.title,
    }
}

fn note_page_response(page: NotePage) -> NotePageResponse {
    NotePageResponse {
        notes: page.notes.into_iter().map(note_summary_response).collect(),
        next_cursor: next_cursor(page.next_offset),
    }
}

async fn note_source(
    State(state): State<ApiState>,
    Path(note_id): Path<String>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let actor = authenticated_actor(&headers, &state).await?;
    let note_id = NoteId::new(
        EntityId::from_str(&note_id)
            .map_err(|_| ApiError::new(ApiErrorCode::NotFound, "note is not available"))?,
    );
    let source = state
        .notes
        .read_source(actor, note_id)
        .await
        .map_err(|error| note_error(error, "note lookup is unavailable"))?;
    let mut response = source.content.into_response();
    response.headers_mut().insert(
        header::ETAG,
        HeaderValue::from_str(&etag(source.revision))
            .map_err(|_| ApiError::new(ApiErrorCode::Internal, "note lookup is unavailable"))?,
    );
    Ok(response)
}

async fn note_metadata(
    State(state): State<ApiState>,
    Path(note_id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<NoteMetadataResponse>, ApiError> {
    let actor = authenticated_actor(&headers, &state).await?;
    let note_id = NoteId::new(
        EntityId::from_str(&note_id)
            .map_err(|_| ApiError::new(ApiErrorCode::NotFound, "note is not available"))?,
    );
    let source = state
        .notes
        .read_source(actor, note_id)
        .await
        .map_err(|error| note_error(error, "note lookup is unavailable"))?;
    Ok(Json(NoteMetadataResponse {
        note_id: source.note_id.to_string(),
        title: source.title,
        revision: source.revision.to_hex(),
    }))
}

async fn update_note_source(
    State(state): State<ApiState>,
    Path(note_id): Path<String>,
    headers: HeaderMap,
    source: String,
) -> Result<StatusCode, ApiError> {
    let actor = authenticated_actor(&headers, &state).await?;
    require_csrf(&headers, &state).await?;
    let note_id = NoteId::new(
        EntityId::from_str(&note_id)
            .map_err(|_| ApiError::new(ApiErrorCode::NotFound, "note is not available"))?,
    );
    let expected_revision = required_if_match(&headers)?;
    state
        .notes
        .update_source(actor, note_id, source, expected_revision)
        .await
        .map_err(|error| note_error(error, "note update is unavailable"))?;
    Ok(StatusCode::NO_CONTENT)
}

async fn delete_note(
    State(state): State<ApiState>,
    Path(note_id): Path<String>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    let actor = authenticated_actor(&headers, &state).await?;
    require_csrf(&headers, &state).await?;
    let note_id = NoteId::new(
        EntityId::from_str(&note_id)
            .map_err(|_| ApiError::new(ApiErrorCode::NotFound, "note is not available"))?,
    );
    let expected_revision = required_if_match(&headers)?;
    state
        .notes
        .delete_note(actor, note_id, expected_revision)
        .await
        .map_err(|error| note_error(error, "note deletion is unavailable"))?;
    Ok(StatusCode::NO_CONTENT)
}

async fn create_note(
    State(state): State<ApiState>,
    headers: HeaderMap,
    source: String,
) -> Result<Response, ApiError> {
    let actor = authenticated_actor(&headers, &state).await?;
    require_csrf(&headers, &state).await?;
    let note_id = state
        .notes
        .create_source(actor, source)
        .await
        .map_err(|error| note_error(error, "note creation is unavailable"))?;
    let mut headers = HeaderMap::new();
    headers.insert(
        header::LOCATION,
        HeaderValue::from_str(&format!("/api/v1/notes/{note_id}/source"))
            .map_err(|_| ApiError::new(ApiErrorCode::Internal, "note creation is unavailable"))?,
    );
    Ok((StatusCode::CREATED, headers).into_response())
}

#[derive(Deserialize)]
struct AclUpdateRequest {
    permission: Option<String>,
}

async fn update_note_acl(
    State(state): State<ApiState>,
    Path((note_id, user_id)): Path<(String, String)>,
    headers: HeaderMap,
    Json(request): Json<AclUpdateRequest>,
) -> Result<StatusCode, ApiError> {
    let actor = authenticated_actor(&headers, &state).await?;
    require_csrf(&headers, &state).await?;
    let note_id = NoteId::new(
        EntityId::from_str(&note_id)
            .map_err(|_| ApiError::new(ApiErrorCode::NotFound, "note is not available"))?,
    );
    let user_id = UserId::new(
        EntityId::from_str(&user_id)
            .map_err(|_| ApiError::new(ApiErrorCode::ValidationFailed, "user ID is invalid"))?,
    );
    let permission = match request.permission.as_deref() {
        Some("read") => Some(NotePermission::Read),
        Some("write") => Some(NotePermission::Write),
        Some("admin") => Some(NotePermission::Admin),
        None => None,
        Some(_) => {
            return Err(ApiError::new(
                ApiErrorCode::ValidationFailed,
                "permission is invalid",
            ));
        }
    };
    state
        .notes
        .set_permission(actor, note_id, user_id, permission)
        .await
        .map_err(|error| note_error(error, "note ACL update was rejected"))?;
    Ok(StatusCode::NO_CONTENT)
}

async fn begin_oidc_login(State(state): State<ApiState>) -> Result<Redirect, ApiError> {
    let destination = state
        .authentication
        .begin_oidc_login()
        .await
        .map_err(authentication_error)?;
    Ok(Redirect::temporary(&destination))
}

#[derive(Deserialize)]
struct OidcCallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

#[derive(Deserialize)]
struct RootLoginRequest {
    password: String,
}

#[derive(Serialize)]
struct PendingOidcUserResponse {
    user_id: String,
    display_name: String,
}

async fn complete_oidc_login(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Query(query): Query<OidcCallbackQuery>,
) -> Result<Response, ApiError> {
    if query.error.is_some() {
        tracing::warn!("OIDC callback rejected by authorization server");
        return Err(ApiError::new(
            ApiErrorCode::AuthenticationRequired,
            "authentication failed",
        ));
    }
    let (Some(code), Some(state_token)) = (query.code.as_deref(), query.state.as_deref()) else {
        tracing::warn!("OIDC callback rejected: missing authorization response parameters");
        return Err(ApiError::new(
            ApiErrorCode::AuthenticationRequired,
            "authentication failed",
        ));
    };
    let OidcLoginResult::Active(user) = state
        .authentication
        .complete_oidc_login(code.into(), state_token.into())
        .await
        .map_err(authentication_error)?
    else {
        tracing::warn!("OIDC callback rejected: user is not authorized");
        return Err(ApiError::new(
            ApiErrorCode::AuthenticationRequired,
            "authentication failed",
        ));
    };
    let session = state
        .authentication
        .issue_oidc_session(user.user_id)
        .await
        .map_err(authentication_error)?;
    let cookie_path = state.authentication.cookie_path();
    let mut response_headers = session_headers(&session, cookie_path)?;
    let destination = oidc_return_to(&headers)
        .unwrap_or_else(|| format!("{}/", cookie_path.trim_end_matches('/')));
    response_headers.append(
        header::SET_COOKIE,
        HeaderValue::from_str(&format!(
            "marginalis_oauth_return_to=; Path={cookie_path}; Max-Age=0; Secure; HttpOnly; SameSite=Lax"
        ))
        .map_err(|_| ApiError::new(ApiErrorCode::Internal, "authentication is unavailable"))?,
    );
    Ok((response_headers, Redirect::to(&destination)).into_response())
}

fn oidc_return_to(headers: &HeaderMap) -> Option<String> {
    let encoded = cookie_value(headers, "marginalis_oauth_return_to")?;
    let decoded = url::form_urlencoded::parse(format!("value={encoded}").as_bytes())
        .find_map(|(key, value)| (key == "value").then(|| value.into_owned()))?;
    decoded.starts_with("/oauth/authorize?").then_some(decoded)
}

async fn root_login(
    State(state): State<ApiState>,
    Json(request): Json<RootLoginRequest>,
) -> Result<Response, ApiError> {
    let session = state
        .authentication
        .root_login(request.password)
        .await
        .map_err(authentication_error)?;
    let Some(session) = session else {
        tracing::warn!("root login rejected");
        return Err(ApiError::new(
            ApiErrorCode::AuthenticationRequired,
            "authentication failed",
        ));
    };
    let cookie_path = state.authentication.cookie_path();
    let mut response = StatusCode::NO_CONTENT.into_response();
    response
        .headers_mut()
        .extend(session_headers(&session, cookie_path)?);
    tracing::info!("root login accepted");
    Ok(response)
}

async fn list_pending_users(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Result<Json<Vec<PendingOidcUserResponse>>, ApiError> {
    require_root(&headers, &state).await?;
    let users = state
        .authentication
        .list_pending_users()
        .await
        .map_err(authentication_error)?;
    Ok(Json(users.into_iter().map(pending_user_response).collect()))
}

async fn activate_pending_user(
    State(state): State<ApiState>,
    Path(user_id): Path<String>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    require_root(&headers, &state).await?;
    require_csrf(&headers, &state).await?;
    let user_id = UserId::new(
        EntityId::from_str(&user_id)
            .map_err(|_| ApiError::new(ApiErrorCode::NotFound, "user is not available"))?,
    );
    let activated = state
        .authentication
        .activate_pending_user(user_id)
        .await
        .map_err(authentication_error)?;
    if !activated {
        return Err(ApiError::new(
            ApiErrorCode::NotFound,
            "user is not available",
        ));
    }
    tracing::info!(user_id = %user_id, "OIDC user activated by root");
    Ok(StatusCode::NO_CONTENT)
}

fn pending_user_response(user: OidcUser) -> PendingOidcUserResponse {
    PendingOidcUserResponse {
        user_id: user.user_id.to_string(),
        display_name: user.display_name,
    }
}

/// Cookie sessionをapplication actorへ変換する唯一のHTTP境界。
async fn authenticated_actor(headers: &HeaderMap, state: &ApiState) -> Result<Actor, ApiError> {
    let session_id = cookie_value(headers, "marginalis_session").ok_or(ApiError::new(
        ApiErrorCode::AuthenticationRequired,
        "authentication is required",
    ))?;
    let session = state
        .authentication
        .authenticate_session(session_id)
        .await
        .map_err(authentication_error)?
        .ok_or(ApiError::new(
            ApiErrorCode::AuthenticationRequired,
            "authentication is required",
        ))?;
    Ok(session.actor)
}

async fn require_root(headers: &HeaderMap, state: &ApiState) -> Result<Actor, ApiError> {
    let actor = authenticated_actor(headers, state).await?;
    if actor.is_root {
        Ok(actor)
    } else {
        Err(ApiError::new(
            ApiErrorCode::Forbidden,
            "root access is required",
        ))
    }
}

fn session_headers(session: &WebSession, cookie_path: &str) -> Result<HeaderMap, ApiError> {
    let cookie = format!(
        "marginalis_session={}; Path={cookie_path}; Secure; HttpOnly; SameSite=Lax",
        session.session_id,
    );
    let csrf_cookie = format!(
        "marginalis_csrf={}; Path={cookie_path}; Secure; SameSite=Lax",
        session.csrf_token,
    );
    let mut headers = HeaderMap::new();
    headers.insert(
        header::SET_COOKIE,
        HeaderValue::from_str(&cookie)
            .map_err(|_| ApiError::new(ApiErrorCode::Internal, "authentication is unavailable"))?,
    );
    headers.append(
        header::SET_COOKIE,
        HeaderValue::from_str(&csrf_cookie)
            .map_err(|_| ApiError::new(ApiErrorCode::Internal, "authentication is unavailable"))?,
    );
    Ok(headers)
}

async fn require_csrf(headers: &HeaderMap, state: &ApiState) -> Result<(), ApiError> {
    let csrf_token = headers
        .get("x-csrf-token")
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or(ApiError::new(
            ApiErrorCode::Forbidden,
            "CSRF token is required",
        ))?;
    require_csrf_token(headers, state, csrf_token).await
}

async fn require_csrf_token(
    headers: &HeaderMap,
    state: &ApiState,
    csrf_token: String,
) -> Result<(), ApiError> {
    let session_id = cookie_value(headers, "marginalis_session").ok_or(ApiError::new(
        ApiErrorCode::AuthenticationRequired,
        "authentication is required",
    ))?;
    let valid = state
        .authentication
        .verify_csrf(session_id, csrf_token)
        .await
        .map_err(authentication_error)?;
    if valid {
        Ok(())
    } else {
        Err(ApiError::new(
            ApiErrorCode::Forbidden,
            "CSRF token is invalid",
        ))
    }
}

async fn logout(State(state): State<ApiState>, headers: HeaderMap) -> Result<Response, ApiError> {
    let _actor = authenticated_actor(&headers, &state).await?;
    require_csrf(&headers, &state).await?;
    let session_id = cookie_value(&headers, "marginalis_session").ok_or(ApiError::new(
        ApiErrorCode::AuthenticationRequired,
        "authentication is required",
    ))?;
    state
        .authentication
        .revoke_session(session_id)
        .await
        .map_err(authentication_error)?;
    let mut response = StatusCode::NO_CONTENT.into_response();
    response.headers_mut().insert(
        header::SET_COOKIE,
        HeaderValue::from_static(
            "marginalis_session=; Path=/; Max-Age=0; Secure; HttpOnly; SameSite=Lax",
        ),
    );
    Ok(response)
}

fn cookie_value(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .find_map(|pair| {
            let (key, value) = pair.trim().split_once('=')?;
            (key == name && !value.is_empty()).then(|| value.to_owned())
        })
}

#[cfg(test)]
mod tests {
    use axum::{
        body::Body,
        http::{Request, StatusCode},
        response::IntoResponse,
    };
    use marginalis_application::{
        McpAuthorizationRequest, McpOAuthUseCaseError, McpOAuthUseCases, McpTokenPair,
        OidcIdentityStore, RootCredentialStore, WebSession, WebSessionStore,
    };
    use marginalis_domain::{
        Actor, EntityId, McpOAuthClient, OidcIdentity, RegistrationPolicy, UnixMillis, UserId,
    };
    use std::{str::FromStr, sync::Arc};
    use tower::ServiceExt;

    use super::{
        ApiError, ApiErrorCode, ApiState, McpEndpoint, McpRateLimiter, OidcConfiguration,
        OidcConfigurationError, router,
    };
    use marginalis_mcp::{McpAuthenticationError, McpAuthenticator, McpTools};

    struct RejectMcpAuthenticator;

    #[async_trait::async_trait]
    impl McpAuthenticator for RejectMcpAuthenticator {
        async fn authenticate(
            &self,
            _bearer_token: &str,
            _required_scope: &str,
        ) -> Result<Actor, McpAuthenticationError> {
            Err(McpAuthenticationError::MissingOrInvalid)
        }
    }

    struct RejectMcpOAuth;

    #[async_trait::async_trait]
    impl McpOAuthUseCases for RejectMcpOAuth {
        async fn validate_authorization_request(
            &self,
            _request: McpAuthorizationRequest,
        ) -> Result<McpOAuthClient, McpOAuthUseCaseError> {
            Err(McpOAuthUseCaseError::Rejected)
        }

        async fn authorize(
            &self,
            _actor: Actor,
            _request: McpAuthorizationRequest,
        ) -> Result<String, McpOAuthUseCaseError> {
            Err(McpOAuthUseCaseError::Rejected)
        }

        async fn exchange_authorization_code(
            &self,
            _code: String,
            _client_id: String,
            _redirect_uri: String,
            _resource_uri: String,
            _code_verifier: String,
        ) -> Result<McpTokenPair, McpOAuthUseCaseError> {
            Err(McpOAuthUseCaseError::Rejected)
        }

        async fn refresh_access_token(
            &self,
            _refresh_token: String,
            _client_id: String,
            _resource_uri: String,
        ) -> Result<McpTokenPair, McpOAuthUseCaseError> {
            Err(McpOAuthUseCaseError::Rejected)
        }
    }

    #[test]
    fn oidc_configuration_preserves_the_base_url_subpath_in_its_callback() {
        let configuration = OidcConfiguration::new(
            "https://identity.example.edu".into(),
            "marginalis".into(),
            "secret".into(),
            "https://example.edu/marginalis/",
        )
        .expect("valid OIDC configuration");

        assert_eq!(
            configuration.redirect_url().as_str(),
            "https://example.edu/marginalis/auth/oidc/callback"
        );
        assert_eq!(
            configuration.issuer_url().as_str(),
            "https://identity.example.edu"
        );
        assert_eq!(configuration.client_id().as_str(), "marginalis");
    }

    #[test]
    fn mcp_rate_limiter_rejects_the_next_request_in_a_window() {
        let limiter = McpRateLimiter::new(2);
        let actor = Actor {
            user_id: UserId::new(
                EntityId::from_str("01800000-0000-7000-8000-000000000081").expect("UUIDv7"),
            ),
            is_root: false,
        };
        assert!(limiter.allow(actor));
        assert!(limiter.allow(actor));
        assert!(!limiter.allow(actor));
    }

    #[tokio::test]
    async fn mcp_unauthorized_response_advertises_resource_metadata() {
        let database = marginalis_sqlite::SqliteDatabase::connect("sqlite::memory:")
            .await
            .expect("database");
        let directory = std::env::temp_dir().join("marginalis-web-mcp-contract-test");
        let notes = Arc::new(marginalis_server::ServerNoteUseCases::new(
            database.clone(),
            marginalis_files::FileNoteStore::open(&directory).expect("sources"),
        ));
        let app = router(
            ApiState::with_test_adapters(database, notes.clone()).with_mcp(McpEndpoint {
                tools: McpTools::new(notes),
                authenticator: Arc::new(RejectMcpAuthenticator),
                oauth: Arc::new(RejectMcpOAuth),
                resource_uri: "https://example.test/marginalis/mcp".into(),
                metadata_uri:
                    "https://example.test/marginalis/.well-known/oauth-protected-resource/mcp"
                        .into(),
                authorization_server_uri: "https://example.test/marginalis".into(),
                authorization_endpoint_uri: "https://example.test/marginalis/oauth/authorize"
                    .into(),
                token_endpoint_uri: "https://example.test/marginalis/oauth/token".into(),
                allowed_origin: "https://example.test".into(),
                rate_limiter: McpRateLimiter::new(10),
            }),
        );
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/mcp")
                    .header("accept", "application/json, text/event-stream")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#,
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            response.headers().get("www-authenticate").expect("header"),
            "Bearer resource_metadata=\"https://example.test/marginalis/.well-known/oauth-protected-resource/mcp\""
        );
    }

    #[test]
    fn oidc_configuration_rejects_an_insecure_or_credentialed_base_url() {
        for base_url in ["http://localhost:3000/", "https://user@example.edu/"] {
            let result = OidcConfiguration::new(
                "https://identity.example.edu".into(),
                "marginalis".into(),
                "secret".into(),
                base_url,
            );
            assert!(matches!(
                result,
                Err(OidcConfigurationError::InvalidBaseUrl)
            ));
        }
    }

    #[tokio::test]
    async fn health_endpoint_is_available_under_the_versioned_api_prefix() {
        let database = marginalis_sqlite::SqliteDatabase::connect("sqlite::memory:")
            .await
            .expect("open store");
        let directory = std::env::temp_dir().join("marginalis-web-health-test");
        let sources = marginalis_files::FileNoteStore::open(&directory).expect("open sources");
        let response = router(ApiState::with_test_adapters(
            database.clone(),
            Arc::new(marginalis_server::ServerNoteUseCases::new(
                database, sources,
            )),
        ))
        .oneshot(
            Request::builder()
                .uri("/api/v1/health")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn landing_endpoint_is_available_after_oidc_redirect() {
        let database = marginalis_sqlite::SqliteDatabase::connect("sqlite::memory:")
            .await
            .expect("open store");
        let directory = std::env::temp_dir().join("marginalis-web-landing-test");
        let sources = marginalis_files::FileNoteStore::open(&directory).expect("open sources");
        let response = router(ApiState::with_test_adapters(
            database.clone(),
            Arc::new(marginalis_server::ServerNoteUseCases::new(
                database, sources,
            )),
        ))
        .oneshot(
            Request::builder()
                .uri("/")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn note_source_requires_an_authenticated_session() {
        let database = marginalis_sqlite::SqliteDatabase::connect("sqlite::memory:")
            .await
            .expect("open database");
        let directory = std::env::temp_dir().join("marginalis-web-note-auth-test");
        let sources = marginalis_files::FileNoteStore::open(&directory).expect("open sources");
        let response = router(ApiState::with_test_adapters(
            database.clone(),
            Arc::new(marginalis_server::ServerNoteUseCases::new(
                database, sources,
            )),
        ))
        .oneshot(
            Request::builder()
                .uri("/api/v1/notes/01800000-0000-7000-8000-000000000001/source")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn note_creation_requires_a_matching_csrf_token() {
        let database = marginalis_sqlite::SqliteDatabase::connect("sqlite::memory:")
            .await
            .expect("open database");
        let user_id = UserId::new(
            EntityId::from_str("01800000-0000-7000-8000-000000000041").expect("UUIDv7"),
        );
        sqlx::query("INSERT INTO users (user_id, authentication_kind, status, display_name, created_at_ms, updated_at_ms) VALUES (?, 'oidc', 'active', 'User', 0, 0)")
            .bind(user_id.to_string()).execute(database.pool()).await.expect("user");
        database
            .web_session_store()
            .issue(
                WebSession {
                    session_id: "session".into(),
                    csrf_token: "csrf".into(),
                    actor: Actor {
                        user_id,
                        is_root: false,
                    },
                    idle_expires_at: UnixMillis::new(4_000_000_000_000),
                    absolute_expires_at: UnixMillis::new(4_000_000_000_000),
                },
                UnixMillis::new(0),
            )
            .await
            .expect("session");
        let directory = std::env::temp_dir().join("marginalis-web-csrf-test");
        let sources = marginalis_files::FileNoteStore::open(&directory).expect("open sources");
        let response = router(ApiState::with_test_adapters(
            database.clone(),
            Arc::new(marginalis_server::ServerNoteUseCases::new(
                database, sources,
            )),
        ))
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/notes")
                .header("cookie", "marginalis_session=session")
                .body(Body::from("invalid source"))
                .expect("request"),
        )
        .await
        .expect("response");
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn root_can_log_in_and_activate_a_pending_oidc_user() {
        let database = marginalis_sqlite::SqliteDatabase::connect("sqlite::memory:")
            .await
            .expect("open database");
        let root_id = UserId::new(
            EntityId::from_str("01800000-0000-7000-8000-000000000050").expect("UUIDv7"),
        );
        database
            .root_credential_store()
            .initialize_if_missing("root-password".into(), root_id, UnixMillis::new(0))
            .await
            .expect("initialize root");
        let pending_id = UserId::new(
            EntityId::from_str("01800000-0000-7000-8000-000000000051").expect("UUIDv7"),
        );
        database
            .oidc_identity_store()
            .register_or_lookup(
                OidcIdentity::new("https://id.example.test", "pending", "Pending")
                    .expect("identity"),
                RegistrationPolicy::Approval,
                pending_id,
                UnixMillis::new(0),
            )
            .await
            .expect("register pending user");
        let directory = std::env::temp_dir().join("marginalis-web-root-admin-test");
        let sources = marginalis_files::FileNoteStore::open(&directory).expect("open sources");
        let app = router(ApiState::with_test_adapters(
            database.clone(),
            Arc::new(marginalis_server::ServerNoteUseCases::new(
                database, sources,
            )),
        ));

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/auth/root/login")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"password":"root-password"}"#))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        let cookies: Vec<_> = response
            .headers()
            .get_all("set-cookie")
            .iter()
            .map(|value| value.to_str().expect("cookie").to_owned())
            .collect();
        let session = cookie_from_set_cookie(&cookies, "marginalis_session");
        let csrf = cookie_from_set_cookie(&cookies, "marginalis_csrf");

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/admin/users/pending")
                    .header("cookie", format!("marginalis_session={session}"))
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/session")
                    .header("cookie", format!("marginalis_session={session}"))
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);

        let response = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/admin/users/{pending_id}/activate"))
                    .header("cookie", format!("marginalis_session={session}"))
                    .header("x-csrf-token", csrf)
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
    }

    fn cookie_from_set_cookie(values: &[String], name: &str) -> String {
        values
            .iter()
            .find_map(|value| {
                let (key, value) = value.split_once('=')?;
                (key == name)
                    .then_some(value.split(';').next())
                    .flatten()
                    .map(str::to_owned)
            })
            .expect("cookie is set")
    }

    #[test]
    fn api_errors_have_stable_http_statuses() {
        let response =
            ApiError::new(ApiErrorCode::ValidationFailed, "invalid input").into_response();

        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }
}
