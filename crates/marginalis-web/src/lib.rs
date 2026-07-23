//! MarginalisのWeb APIにおけるHTTP境界。
//!
//! 認証、Web UIおよびMCPはこのcrateのHTTP adapterとして追加する。ノートの検証、ACLおよび
//! 永続化の業務判断は`marginalis-application`のユースケースへ委譲する。

use std::{str::FromStr, sync::Arc};

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Redirect, Response},
    routing::{delete, get, post, put},
};
use marginalis_application::{
    Clock, NoteUseCaseError, NoteUseCases, OidcUserAdministrationStore, RootCredentialStore,
    SessionLifetime, WebSession, WebSessionService, WebSessionStore,
};
pub use marginalis_auth_oidc::{
    OidcAuthentication, OidcCallbackError, OidcCallbackRejection, OidcConfiguration,
    OidcConfigurationError, OidcDiscoveryError, OidcLoginStartError,
};
use marginalis_domain::{
    Actor, EntityId, NoteId, NotePermission, NoteSummary, OidcLoginResult,
    OidcUser, UserId,
};
use marginalis_server::{SystemClock, SystemRandom};
use marginalis_sqlite::SqliteDatabase;
use serde::{Deserialize, Serialize};
use tower_http::trace::{DefaultOnResponse, TraceLayer};
use tracing::Level;
use tracing::info_span;

/// 公開REST APIの現在のバージョン。
pub const API_VERSION: &str = "v1";

/// HTTPハンドラーが利用する共有状態。
#[derive(Clone)]
pub struct ApiState {
    pub database: SqliteDatabase,
    pub notes: Arc<dyn NoteUseCases>,
    pub oidc: Option<Arc<OidcAuthentication>>,
}

impl ApiState {
    pub fn new(database: SqliteDatabase, notes: Arc<dyn NoteUseCases>) -> Self {
        Self {
            database,
            notes,
            oidc: None,
        }
    }

    pub fn with_oidc(
        database: SqliteDatabase,
        notes: Arc<dyn NoteUseCases>,
        oidc: OidcAuthentication,
    ) -> Self {
        Self {
            database,
            notes,
            oidc: Some(Arc::new(oidc)),
        }
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
        .route("/api/v1/notes/{note_id}/source", put(update_note_source))
        .route("/api/v1/notes/{note_id}", delete(delete_note))
        .route("/api/v1/notes", get(list_notes).post(create_note))
        .route("/api/v1/search", get(search_notes))
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
}

#[derive(Deserialize)]
struct NoteSearchQuery {
    q: String,
    limit: Option<u32>,
}

#[derive(Serialize)]
struct NoteSummaryResponse {
    note_id: String,
    title: String,
}

#[derive(Serialize)]
struct NoteSearchResponse {
    note_id: String,
    title: String,
}

async fn list_notes(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Query(query): Query<NoteListQuery>,
) -> Result<Json<Vec<NoteSummaryResponse>>, ApiError> {
    let actor = authenticated_actor(&headers, &state).await?;
    let notes = state
        .notes
        .list_notes(actor, bounded_limit(query.limit))
        .await
        .map_err(|error| note_error(error, "note listing is unavailable"))?;
    Ok(Json(notes.into_iter().map(note_summary_response).collect()))
}

async fn search_notes(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Query(query): Query<NoteSearchQuery>,
) -> Result<Json<Vec<NoteSearchResponse>>, ApiError> {
    let actor = authenticated_actor(&headers, &state).await?;
    let results = state
        .notes
        .search_notes(actor, query.q, bounded_limit(query.limit))
        .await
        .map_err(|error| note_error(error, "note search is unavailable"))?;
    Ok(Json(
        results.into_iter().map(note_search_response).collect(),
    ))
}

fn bounded_limit(value: Option<u32>) -> u32 {
    match value.unwrap_or(50) {
        0 => 1,
        value if value > 100 => 100,
        value => value,
    }
}

fn note_summary_response(note: NoteSummary) -> NoteSummaryResponse {
    NoteSummaryResponse {
        note_id: note.note_id.to_string(),
        title: note.title,
    }
}

fn note_search_response(note: NoteSummary) -> NoteSearchResponse {
    NoteSearchResponse {
        note_id: note.note_id.to_string(),
        title: note.title,
    }
}

async fn note_source(
    State(state): State<ApiState>,
    Path(note_id): Path<String>,
    headers: HeaderMap,
) -> Result<Vec<u8>, ApiError> {
    let actor = authenticated_actor(&headers, &state).await?;
    let note_id = NoteId::new(
        EntityId::from_str(&note_id)
            .map_err(|_| ApiError::new(ApiErrorCode::NotFound, "note is not available"))?,
    );
    state
        .notes
        .read_source(actor, note_id)
        .await
        .map_err(|error| note_error(error, "note lookup is unavailable"))
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
    state
        .notes
        .update_source(actor, note_id, source)
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
    state
        .notes
        .delete_note(actor, note_id)
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
    let oidc = state.oidc.as_ref().ok_or(ApiError::new(
        ApiErrorCode::Internal,
        "authentication is not configured",
    ))?;
    let destination = oidc
        .begin_login(
            &state.database.oidc_login_attempt_store(),
            &SystemRandom,
            &SystemClock,
        )
        .await
        .map_err(|_| ApiError::new(ApiErrorCode::Internal, "authentication is unavailable"))?;
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
    Query(query): Query<OidcCallbackQuery>,
) -> Result<Response, ApiError> {
    let oidc = state.oidc.as_ref().ok_or(ApiError::new(
        ApiErrorCode::Internal,
        "authentication is not configured",
    ))?;
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
    let OidcLoginResult::Active(user) = oidc
        .complete_login(
            &state.database.oidc_login_attempt_store(),
            &state.database.oidc_identity_store(),
            &SystemRandom,
            &SystemClock,
            code,
            state_token,
        )
        .await
        .map_err(|error| match error {
            OidcCallbackError::Rejected(_) => {
                tracing::warn!(stage = error.diagnostic_stage(), "OIDC callback rejected");
                ApiError::new(
                    ApiErrorCode::AuthenticationRequired,
                    "authentication failed",
                )
            }
            OidcCallbackError::Unavailable => {
                ApiError::new(ApiErrorCode::Internal, "authentication is unavailable")
            }
        })?
    else {
        tracing::warn!("OIDC callback rejected: user is not authorized");
        return Err(ApiError::new(
            ApiErrorCode::AuthenticationRequired,
            "authentication failed",
        ));
    };
    let clock = SystemClock;
    let random = SystemRandom;
    let session = WebSessionService::new(&state.database.web_session_store(), &random, &clock)
        .issue(
            Actor {
                user_id: user.user_id,
                is_root: false,
            },
            SessionLifetime {
                idle_timeout_ms: 8 * 60 * 60 * 1_000,
                absolute_timeout_ms: 7 * 24 * 60 * 60 * 1_000,
            },
        )
        .await
        .map_err(|_| ApiError::new(ApiErrorCode::Internal, "authentication is unavailable"))?;
    let headers = session_headers(&session, oidc.cookie_path())?;
    Ok((
        headers,
        Redirect::to(&format!("{}/", oidc.cookie_path().trim_end_matches('/'))),
    )
        .into_response())
}

async fn root_login(
    State(state): State<ApiState>,
    Json(request): Json<RootLoginRequest>,
) -> Result<Response, ApiError> {
    let root_user_id = state
        .database
        .root_credential_store()
        .verify_password(request.password)
        .await
        .map_err(|_| ApiError::new(ApiErrorCode::Internal, "authentication is unavailable"))?;
    let Some(user_id) = root_user_id else {
        tracing::warn!("root login rejected");
        return Err(ApiError::new(
            ApiErrorCode::AuthenticationRequired,
            "authentication failed",
        ));
    };
    let session = WebSessionService::new(
        &state.database.web_session_store(),
        &SystemRandom,
        &SystemClock,
    )
    .issue(
        Actor {
            user_id,
            is_root: true,
        },
        SessionLifetime {
            idle_timeout_ms: 30 * 60 * 1_000,
            absolute_timeout_ms: 8 * 60 * 60 * 1_000,
        },
    )
    .await
    .map_err(|_| ApiError::new(ApiErrorCode::Internal, "authentication is unavailable"))?;
    let cookie_path = state
        .oidc
        .as_ref()
        .map(|oidc| oidc.cookie_path())
        .unwrap_or("/");
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
        .database
        .oidc_user_administration_store()
        .list_pending()
        .await
        .map_err(|_| ApiError::new(ApiErrorCode::Internal, "user lookup is unavailable"))?;
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
        .database
        .oidc_user_administration_store()
        .activate(user_id, SystemClock.now())
        .await
        .map_err(|_| ApiError::new(ApiErrorCode::Internal, "user update is unavailable"))?;
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
        .database
        .web_session_store()
        .lookup(session_id, SystemClock.now())
        .await
        .map_err(|_| ApiError::new(ApiErrorCode::Internal, "authentication is unavailable"))?
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
    let session_id = cookie_value(headers, "marginalis_session").ok_or(ApiError::new(
        ApiErrorCode::AuthenticationRequired,
        "authentication is required",
    ))?;
    let csrf_token = headers
        .get("x-csrf-token")
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or(ApiError::new(
            ApiErrorCode::Forbidden,
            "CSRF token is required",
        ))?;
    let valid = state
        .database
        .web_session_store()
        .verify_csrf(session_id, csrf_token, SystemClock.now())
        .await
        .map_err(|_| ApiError::new(ApiErrorCode::Internal, "authentication is unavailable"))?;
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
        .database
        .web_session_store()
        .revoke(session_id, SystemClock.now())
        .await
        .map_err(|_| ApiError::new(ApiErrorCode::Internal, "authentication is unavailable"))?;
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
        OidcIdentityStore, RootCredentialStore, WebSession, WebSessionStore,
    };
    use marginalis_domain::{
        Actor, EntityId, OidcIdentity, RegistrationPolicy, UnixMillis, UserId,
    };
    use std::{str::FromStr, sync::Arc};
    use tower::ServiceExt;

    use super::{
        ApiError, ApiErrorCode, ApiState, OidcConfiguration, OidcConfigurationError, router,
    };

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
        let response = router(ApiState::new(
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
        let response = router(ApiState::new(
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
        let response = router(ApiState::new(
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
        let response = router(ApiState::new(
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
        let app = router(ApiState::new(
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
