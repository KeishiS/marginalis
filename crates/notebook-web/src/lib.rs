//! Web APIのHTTP境界。
//!
//! 認証、Web UIおよびMCPは将来このcrateのアダプタとして追加する。ノートの検証、ACLおよび
//! 永続化の業務判断は`notebook-store`以下のアプリケーションサービスへ委譲する。

use std::{env, fmt};

use axum::{
    Json, Router,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use notebook_store::{NotebookStore, Viewer};
use openidconnect::{
    ClientId, ClientSecret, EndpointMaybeSet, EndpointNotSet, EndpointSet, IssuerUrl, RedirectUrl,
    core::{CoreClient, CoreProviderMetadata},
    reqwest,
};
use serde::Serialize;
use url::Url;

/// 公開REST APIの現在のバージョン。
pub const API_VERSION: &str = "v1";

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
        Ok(Self {
            issuer_url,
            client_id: ClientId::new(client_id),
            client_secret: ClientSecret::new(client_secret),
            redirect_url,
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
        })
    }

    pub fn client(&self) -> &DiscoveredOidcClient {
        &self.client
    }

    pub fn http_client(&self) -> &reqwest::Client {
        &self.http_client
    }
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

/// HTTPハンドラーが利用する共有状態。
#[derive(Clone, Debug)]
pub struct ApiState {
    pub store: NotebookStore,
}

/// 認証アダプタだけが生成する、リクエストに結び付いた利用者文脈。
///
/// OAuth、Cookie sessionおよび将来のMCP tokenは異なるが、ACL判定に渡す値はこの型へ統一する。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthenticatedActor {
    pub viewer: Viewer,
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
            "notebook".into(),
            "secret".into(),
            "https://example.edu/notebook/",
        )
        .expect("valid OIDC configuration");

        assert_eq!(
            configuration.redirect_url().as_str(),
            "https://example.edu/notebook/auth/oidc/callback"
        );
        assert_eq!(
            configuration.issuer_url().as_str(),
            "https://identity.example.edu"
        );
        assert_eq!(configuration.client_id().as_str(), "notebook");
    }

    #[test]
    fn oidc_configuration_rejects_an_insecure_or_credentialed_base_url() {
        for base_url in ["http://localhost:3000/", "https://user@example.edu/"] {
            let result = OidcConfiguration::new(
                "https://identity.example.edu".into(),
                "notebook".into(),
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
        let store = notebook_store::NotebookStore::connect("sqlite::memory:")
            .await
            .expect("open store");
        let response = router(ApiState { store })
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
