//! MarginalisのWeb APIにおけるHTTP境界。
//!
//! 認証、Web UIおよびMCPはこのcrateのHTTP adapterとして追加する。ノートの検証、ACLおよび
//! 永続化の業務判断は`marginalis-application`のユースケースへ委譲する。

use std::{env, fmt, sync::Arc};

use axum::{
    Json, Router,
    extract::{Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Redirect, Response},
    routing::get,
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use marginalis_application::{
    Clock, OidcLoginAttempt, OidcLoginAttemptStore, OidcRegistrationService, Random,
    SessionLifetime, WebSessionService,
};
use marginalis_domain::{
    Actor, EntityId, OidcIdentity, OidcLoginResult, RegistrationPolicy, UnixMillis,
};
use marginalis_sqlite::SqliteDatabase;
use openidconnect::{
    AuthorizationCode, ClientId, ClientSecret, CsrfToken, EndpointMaybeSet, EndpointNotSet,
    EndpointSet, IssuerUrl, Nonce, PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, Scope,
    TokenResponse,
    core::{CoreAuthenticationFlow, CoreClient, CoreProviderMetadata},
    reqwest,
};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use url::Url;
use uuid::Uuid;

/// 公開REST APIの現在のバージョン。
pub const API_VERSION: &str = "v1";

struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> UnixMillis {
        UnixMillis::new(OffsetDateTime::now_utc().unix_timestamp_nanos() as i64 / 1_000_000)
    }
}

struct SystemRandom;

impl Random for SystemRandom {
    fn uuid_v7(&self) -> EntityId {
        EntityId::from_uuid_v7(Uuid::now_v7())
    }

    fn opaque_token(&self) -> String {
        let bytes: [u8; 32] = rand::random();
        URL_SAFE_NO_PAD.encode(bytes)
    }
}

/// Discovery済みの外部OIDCクライアント。
pub type DiscoveredOidcClient = CoreClient<
    EndpointSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointMaybeSet,
    EndpointMaybeSet,
>;

/// Discovery済みOIDCクライアントと、redirectを追跡しないHTTPクライアント。
///
/// issuerは起動設定で固定される。Discovery応答に含まれるURLを任意のredirect経由で追わない
/// ことで、設定されたIdP以外への意図しないリクエストを防ぐ。
#[derive(Clone)]
pub struct OidcAuthentication {
    client: DiscoveredOidcClient,
    http_client: reqwest::Client,
    cookie_path: String,
}

/// OIDC Discoveryの起動時エラー。詳細な応答本文やsecretは公開しない。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OidcDiscoveryError {
    HttpClient,
    Discovery,
}

impl fmt::Display for OidcDiscoveryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::HttpClient => formatter.write_str("OIDC HTTP client could not be initialized"),
            Self::Discovery => formatter.write_str("OIDC Discovery failed"),
        }
    }
}

impl std::error::Error for OidcDiscoveryError {}

/// 外部OIDCプロバイダへ接続するための起動時設定。
///
/// secretはこの型の外へ文字列として公開せず、DBとログへ保存しない。Discovery、認可要求、
/// callback検証はこの設定を受け取る認証アダプタだけが行う。
#[derive(Clone)]
pub struct OidcConfiguration {
    issuer_url: IssuerUrl,
    client_id: ClientId,
    client_secret: ClientSecret,
    redirect_url: RedirectUrl,
    cookie_path: String,
}

/// OIDC設定を起動できない理由。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OidcConfigurationError {
    MissingEnvironment(&'static str),
    InvalidIssuerUrl,
    InvalidBaseUrl,
}

impl fmt::Display for OidcConfigurationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingEnvironment(variable) => {
                write!(
                    formatter,
                    "required environment variable {variable} is not set"
                )
            }
            Self::InvalidIssuerUrl => formatter.write_str("OIDC issuer URL is invalid"),
            Self::InvalidBaseUrl => formatter.write_str("Base URL must be an absolute HTTPS URL"),
        }
    }
}

impl std::error::Error for OidcConfigurationError {}

impl OidcConfiguration {
    /// `OIDC_ISSUER_URL`、`OIDC_CLIENT_ID`および`OIDC_CLIENT_SECRET`を読み込む。
    ///
    /// callback URLはBase URLのサブパスを保った`/auth/oidc/callback`に固定する。
    pub fn from_environment(base_url: &str) -> Result<Self, OidcConfigurationError> {
        Self::new(
            required_environment("OIDC_ISSUER_URL")?,
            required_environment("OIDC_CLIENT_ID")?,
            required_environment("OIDC_CLIENT_SECRET")?,
            base_url,
        )
    }

    pub fn new(
        issuer_url: String,
        client_id: String,
        client_secret: String,
        base_url: &str,
    ) -> Result<Self, OidcConfigurationError> {
        let issuer_url =
            IssuerUrl::new(issuer_url).map_err(|_| OidcConfigurationError::InvalidIssuerUrl)?;
        let redirect_url = oidc_callback_url(base_url)?;
        let cookie_path = base_cookie_path(base_url)?;
        Ok(Self {
            issuer_url,
            client_id: ClientId::new(client_id),
            client_secret: ClientSecret::new(client_secret),
            redirect_url,
            cookie_path,
        })
    }

    pub fn issuer_url(&self) -> &IssuerUrl {
        &self.issuer_url
    }

    pub fn client_id(&self) -> &ClientId {
        &self.client_id
    }

    pub fn redirect_url(&self) -> &RedirectUrl {
        &self.redirect_url
    }

    /// Discovery結果と結合し、認可要求およびcallback検証に使用するクライアントを作る。
    ///
    /// client secretは戻り値の内部にだけ移し、ログ用の文字列へ変換しない。
    pub fn client_from_discovery(
        &self,
        provider_metadata: CoreProviderMetadata,
    ) -> DiscoveredOidcClient {
        CoreClient::from_provider_metadata(
            provider_metadata,
            self.client_id.clone(),
            Some(self.client_secret.clone()),
        )
        .set_redirect_uri(self.redirect_url.clone())
    }
}

impl OidcAuthentication {
    /// 起動時に一度だけDiscoveryとJWKS取得を行う。
    pub async fn discover(configuration: &OidcConfiguration) -> Result<Self, OidcDiscoveryError> {
        let http_client = reqwest::ClientBuilder::new()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| OidcDiscoveryError::HttpClient)?;
        let metadata =
            CoreProviderMetadata::discover_async(configuration.issuer_url().clone(), &http_client)
                .await
                .map_err(|_| OidcDiscoveryError::Discovery)?;
        Ok(Self {
            client: configuration.client_from_discovery(metadata),
            http_client,
            cookie_path: configuration.cookie_path.clone(),
        })
    }

    pub fn client(&self) -> &DiscoveredOidcClient {
        &self.client
    }

    pub fn http_client(&self) -> &reqwest::Client {
        &self.http_client
    }

    /// state、nonceおよびPKCE verifierを保存し、IdPへ転送する認可URLを作る。
    pub async fn begin_login(
        &self,
        database: &SqliteDatabase,
    ) -> Result<String, OidcLoginStartError> {
        let clock = SystemClock;
        let random = SystemRandom;
        let now = clock.now();
        let pending = OidcLoginAttempt {
            state: random.opaque_token(),
            nonce: random.opaque_token(),
            pkce_verifier: random.opaque_token(),
            expires_at: UnixMillis::new(now.get() + 10 * 60 * 1_000),
        };
        database
            .oidc_login_attempt_store()
            .issue(pending.clone())
            .await
            .map_err(|_| OidcLoginStartError::Store)?;
        let verifier = PkceCodeVerifier::new(pending.pkce_verifier);
        let challenge = PkceCodeChallenge::from_code_verifier_sha256(&verifier);
        let state = pending.state;
        let nonce = pending.nonce;
        let (url, _, _) = self
            .client
            .authorize_url(
                CoreAuthenticationFlow::AuthorizationCode,
                move || CsrfToken::new(state),
                move || Nonce::new(nonce),
            )
            .set_pkce_challenge(challenge)
            .add_scope(Scope::new("profile".into()))
            .add_scope(Scope::new("email".into()))
            .url();
        Ok(url.into())
    }

    async fn complete_login(
        &self,
        database: &SqliteDatabase,
        code: &str,
        state: &str,
    ) -> Result<OidcLoginResult, OidcCallbackError> {
        let clock = SystemClock;
        let random = SystemRandom;
        let now = clock.now();
        let pending = database
            .oidc_login_attempt_store()
            .consume(state.to_owned(), now)
            .await
            .map_err(|_| OidcCallbackError::Unavailable)?
            .ok_or(OidcCallbackError::Rejected)?;
        let token = self
            .client
            .exchange_code(AuthorizationCode::new(code.to_owned()))
            .map_err(|_| OidcCallbackError::Rejected)?
            .set_pkce_verifier(PkceCodeVerifier::new(pending.pkce_verifier))
            .request_async(&self.http_client)
            .await
            .map_err(|_| OidcCallbackError::Rejected)?;
        let id_token = token.id_token().ok_or(OidcCallbackError::Rejected)?;
        let claims = id_token
            .claims(&self.client.id_token_verifier(), &Nonce::new(pending.nonce))
            .map_err(|_| OidcCallbackError::Rejected)?;
        let subject = claims.subject().as_str().to_owned();
        let display_name = claims
            .name()
            .and_then(|value| value.get(None))
            .map(|value| value.as_str())
            .or_else(|| claims.preferred_username().map(|value| value.as_str()))
            .or_else(|| claims.email().map(|value| value.as_str()))
            .unwrap_or(&subject)
            .to_owned();
        let identity = OidcIdentity::new(claims.issuer().as_str(), subject, display_name)
            .map_err(|_| OidcCallbackError::Rejected)?;
        OidcRegistrationService::new(&database.oidc_identity_store(), &random)
            .register_or_lookup(identity, RegistrationPolicy::default(), now)
            .await
            .map_err(|_| OidcCallbackError::Unavailable)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OidcLoginStartError {
    Store,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OidcCallbackError {
    Rejected,
    Unavailable,
}

fn required_environment(variable: &'static str) -> Result<String, OidcConfigurationError> {
    env::var(variable).map_err(|_| OidcConfigurationError::MissingEnvironment(variable))
}

fn oidc_callback_url(base_url: &str) -> Result<RedirectUrl, OidcConfigurationError> {
    let mut url = Url::parse(base_url).map_err(|_| OidcConfigurationError::InvalidBaseUrl)?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(OidcConfigurationError::InvalidBaseUrl);
    }
    let base_path = url.path().trim_end_matches('/');
    url.set_path(&format!("{base_path}/auth/oidc/callback"));
    RedirectUrl::new(url.into()).map_err(|_| OidcConfigurationError::InvalidBaseUrl)
}

fn base_cookie_path(base_url: &str) -> Result<String, OidcConfigurationError> {
    let url = Url::parse(base_url).map_err(|_| OidcConfigurationError::InvalidBaseUrl)?;
    let path = url.path().trim_end_matches('/');
    Ok(if path.is_empty() {
        "/".into()
    } else {
        path.into()
    })
}

/// HTTPハンドラーが利用する共有状態。
#[derive(Clone)]
pub struct ApiState {
    pub database: SqliteDatabase,
    pub oidc: Option<Arc<OidcAuthentication>>,
}

impl ApiState {
    pub fn new(database: SqliteDatabase) -> Self {
        Self {
            database,
            oidc: None,
        }
    }

    pub fn with_oidc(database: SqliteDatabase, oidc: OidcAuthentication) -> Self {
        Self {
            database,
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
        .route("/auth/oidc/login", get(begin_oidc_login))
        .route("/auth/oidc/callback", get(complete_oidc_login))
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

async fn begin_oidc_login(State(state): State<ApiState>) -> Result<Redirect, ApiError> {
    let oidc = state.oidc.as_ref().ok_or(ApiError::new(
        ApiErrorCode::Internal,
        "authentication is not configured",
    ))?;
    let destination = oidc
        .begin_login(&state.database)
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
        .complete_login(&state.database, code, state_token)
        .await
        .map_err(|error| match error {
            OidcCallbackError::Rejected => ApiError::new(
                ApiErrorCode::AuthenticationRequired,
                "authentication failed",
            ),
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
        session.session_id, oidc.cookie_path
    );
    let mut headers = HeaderMap::new();
    headers.insert(
        header::SET_COOKIE,
        HeaderValue::from_str(&cookie)
            .map_err(|_| ApiError::new(ApiErrorCode::Internal, "authentication is unavailable"))?,
    );
    Ok((
        headers,
        Redirect::to(&format!("{}/", oidc.cookie_path.trim_end_matches('/'))),
    )
        .into_response())
}

#[cfg(test)]
mod tests {
    use axum::{
        body::Body,
        http::{Request, StatusCode},
        response::IntoResponse,
    };
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
        let response = router(ApiState::new(database))
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

    #[test]
    fn api_errors_have_stable_http_statuses() {
        let response =
            ApiError::new(ApiErrorCode::ValidationFailed, "invalid input").into_response();

        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }
}
