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
    Clock, NoteAclService, NoteAclServiceError, NoteAclStore, NoteOperationKind, NoteWriteService,
    SessionLifetime, WebSessionService, WebSessionStore,
};
pub use marginalis_auth_oidc::{
    OidcAuthentication, OidcCallbackError, OidcCallbackRejection, OidcConfiguration,
    OidcConfigurationError, OidcDiscoveryError, OidcLoginStartError,
};
use marginalis_domain::{Actor, EntityId, NoteId, NotePermission, OidcLoginResult, UserId};
use marginalis_files::FileNoteStore;
use marginalis_server::{SystemClock, SystemRandom};
use marginalis_sqlite::SqliteDatabase;
use serde::{Deserialize, Serialize};

/// 公開REST APIの現在のバージョン。
pub const API_VERSION: &str = "v1";

/// HTTPハンドラーが利用する共有状態。
#[derive(Clone)]
pub struct ApiState {
    pub database: SqliteDatabase,
    pub sources: FileNoteStore,
    pub oidc: Option<Arc<OidcAuthentication>>,
}

impl ApiState {
    pub fn new(database: SqliteDatabase, sources: FileNoteStore) -> Self {
        Self {
            database,
            sources,
            oidc: None,
        }
    }

    pub fn with_oidc(
        database: SqliteDatabase,
        sources: FileNoteStore,
        oidc: OidcAuthentication,
    ) -> Self {
        Self {
            database,
            sources,
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
        .route("/api/v1/health", get(health))
        .route("/api/v1/notes/{note_id}/source", get(note_source))
        .route("/api/v1/notes/{note_id}/source", put(update_note_source))
        .route("/api/v1/notes/{note_id}", delete(delete_note))
        .route("/api/v1/notes", post(create_note))
        .route(
            "/api/v1/notes/{note_id}/acl/{user_id}",
            put(update_note_acl),
        )
        .route("/auth/oidc/login", get(begin_oidc_login))
        .route("/auth/oidc/callback", get(complete_oidc_login))
        .route("/auth/logout", post(logout))
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
    let permission = state
        .database
        .note_acl_store()
        .permission_for(actor, note_id)
        .await
        .map_err(|_| ApiError::new(ApiErrorCode::Internal, "note lookup is unavailable"))?;
    if !matches!(permission, Some(value) if value.permits(NotePermission::Read)) {
        return Err(ApiError::new(
            ApiErrorCode::NotFound,
            "note is not available",
        ));
    }
    state
        .sources
        .read(note_id)
        .map_err(|_| ApiError::new(ApiErrorCode::Internal, "note lookup is unavailable"))?
        .ok_or(ApiError::new(
            ApiErrorCode::NotFound,
            "note is not available",
        ))
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
    let permission = state
        .database
        .note_acl_store()
        .permission_for(actor, note_id)
        .await
        .map_err(|_| ApiError::new(ApiErrorCode::Internal, "note update is unavailable"))?;
    if !matches!(permission, Some(value) if value.permits(NotePermission::Write)) {
        return Err(ApiError::new(
            ApiErrorCode::NotFound,
            "note is not available",
        ));
    }
    let projection = marginalis_asciidoc::parse_note_projection(&source)
        .map_err(|_| ApiError::new(ApiErrorCode::ValidationFailed, "note source is invalid"))?;
    if projection.note_id != note_id {
        return Err(ApiError::new(
            ApiErrorCode::ValidationFailed,
            "note source does not match the requested note",
        ));
    }
    let previous_source = state
        .sources
        .read(note_id)
        .map_err(|_| ApiError::new(ApiErrorCode::Internal, "note update is unavailable"))?
        .ok_or(ApiError::new(
            ApiErrorCode::NotFound,
            "note is not available",
        ))?;
    let previous_source = std::str::from_utf8(&previous_source)
        .map_err(|_| ApiError::new(ApiErrorCode::Internal, "note update is unavailable"))?;
    let previous_projection = marginalis_asciidoc::parse_note_projection(previous_source)
        .map_err(|_| ApiError::new(ApiErrorCode::Internal, "note update is unavailable"))?;
    if projection.owner_id != previous_projection.owner_id {
        return Err(ApiError::new(
            ApiErrorCode::ValidationFailed,
            "note creator cannot be changed",
        ));
    }
    let projections = state.database.note_projection_store();
    let journal = state.database.operation_journal();
    NoteWriteService::new(
        &state.sources,
        &projections,
        &journal,
        &SystemRandom,
        &SystemClock,
    )
    .replace(NoteOperationKind::Update, projection, source.into_bytes())
    .await
    .map_err(|_| ApiError::new(ApiErrorCode::Internal, "note update is unavailable"))?;
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
    let permission = state
        .database
        .note_acl_store()
        .permission_for(actor, note_id)
        .await
        .map_err(|_| ApiError::new(ApiErrorCode::Internal, "note deletion is unavailable"))?;
    if !matches!(permission, Some(value) if value.permits(NotePermission::Admin)) {
        return Err(ApiError::new(
            ApiErrorCode::NotFound,
            "note is not available",
        ));
    }
    let projections = state.database.note_projection_store();
    let journal = state.database.operation_journal();
    NoteWriteService::new(
        &state.sources,
        &projections,
        &journal,
        &SystemRandom,
        &SystemClock,
    )
    .delete(note_id)
    .await
    .map_err(|_| ApiError::new(ApiErrorCode::Internal, "note deletion is unavailable"))?;
    Ok(StatusCode::NO_CONTENT)
}

async fn create_note(
    State(state): State<ApiState>,
    headers: HeaderMap,
    source: String,
) -> Result<Response, ApiError> {
    let actor = authenticated_actor(&headers, &state).await?;
    require_csrf(&headers, &state).await?;
    let projection = marginalis_asciidoc::parse_note_projection(&source)
        .map_err(|_| ApiError::new(ApiErrorCode::ValidationFailed, "note source is invalid"))?;
    if projection.owner_id != actor.user_id {
        return Err(ApiError::new(
            ApiErrorCode::Forbidden,
            "note creator does not match the authenticated user",
        ));
    }
    if state
        .sources
        .read(projection.note_id)
        .map_err(|_| ApiError::new(ApiErrorCode::Internal, "note creation is unavailable"))?
        .is_some()
    {
        return Err(ApiError::new(ApiErrorCode::Conflict, "note already exists"));
    }
    let note_id = projection.note_id.to_string();
    let projections = state.database.note_projection_store();
    let journal = state.database.operation_journal();
    NoteWriteService::new(
        &state.sources,
        &projections,
        &journal,
        &SystemRandom,
        &SystemClock,
    )
    .replace(NoteOperationKind::Create, projection, source.into_bytes())
    .await
    .map_err(|_| ApiError::new(ApiErrorCode::Internal, "note creation is unavailable"))?;
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
    NoteAclService::new(&state.database.note_acl_store())
        .set_permission(actor, note_id, user_id, permission)
        .await
        .map_err(|error| match error {
            NoteAclServiceError::Forbidden => ApiError::new(
                ApiErrorCode::Forbidden,
                "note administration is not permitted",
            ),
            NoteAclServiceError::Store(_) => {
                ApiError::new(ApiErrorCode::Conflict, "note ACL update was rejected")
            }
        })?;
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

async fn complete_oidc_login(
    State(state): State<ApiState>,
    Query(query): Query<OidcCallbackQuery>,
) -> Result<Response, ApiError> {
    let oidc = state.oidc.as_ref().ok_or(ApiError::new(
        ApiErrorCode::Internal,
        "authentication is not configured",
    ))?;
    if query.error.is_some() {
        return Err(ApiError::new(
            ApiErrorCode::AuthenticationRequired,
            "authentication failed",
        ));
    }
    let (Some(code), Some(state_token)) = (query.code.as_deref(), query.state.as_deref()) else {
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
                eprintln!("OIDC callback rejected at {}", error.diagnostic_stage());
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
    let cookie = format!(
        "marginalis_session={}; Path={}; Secure; HttpOnly; SameSite=Lax",
        session.session_id,
        oidc.cookie_path()
    );
    let mut headers = HeaderMap::new();
    headers.insert(
        header::SET_COOKIE,
        HeaderValue::from_str(&cookie)
            .map_err(|_| ApiError::new(ApiErrorCode::Internal, "authentication is unavailable"))?,
    );
    let csrf_cookie = format!(
        "marginalis_csrf={}; Path={}; Secure; SameSite=Lax",
        session.csrf_token,
        oidc.cookie_path()
    );
    headers.append(
        header::SET_COOKIE,
        HeaderValue::from_str(&csrf_cookie)
            .map_err(|_| ApiError::new(ApiErrorCode::Internal, "authentication is unavailable"))?,
    );
    Ok((
        headers,
        Redirect::to(&format!("{}/", oidc.cookie_path().trim_end_matches('/'))),
    )
        .into_response())
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
    use marginalis_application::{WebSession, WebSessionStore};
    use marginalis_domain::{Actor, EntityId, UnixMillis, UserId};
    use std::str::FromStr;
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
        let response = router(ApiState::new(database, sources))
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
    async fn note_source_requires_an_authenticated_session() {
        let database = marginalis_sqlite::SqliteDatabase::connect("sqlite::memory:")
            .await
            .expect("open database");
        let directory = std::env::temp_dir().join("marginalis-web-note-auth-test");
        let sources = marginalis_files::FileNoteStore::open(&directory).expect("open sources");
        let response = router(ApiState::new(database, sources))
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
        let response = router(ApiState::new(database, sources))
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

    #[test]
    fn api_errors_have_stable_http_statuses() {
        let response =
            ApiError::new(ApiErrorCode::ValidationFailed, "invalid input").into_response();

        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }
}
