use serde::{Deserialize, Serialize};

/// Authorization Serverが認証済みとして受け取る主体。
///
/// 本人確認と値の検査は利用側が行う。中核はissuerとsubjectをtokenへ結び付けるためだけに使う。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Principal {
    issuer: String,
    subject: String,
}

impl Principal {
    pub fn new(issuer: String, subject: String) -> Self {
        Self { issuer, subject }
    }

    pub fn issuer(&self) -> &str {
        &self.issuer
    }

    pub fn subject(&self) -> &str {
        &self.subject
    }
}

/// OAuth clientの検証対象となる登録情報。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Client {
    pub client_id: String,
    pub display_name: String,
    pub redirect_uris: Vec<String>,
}

/// DCRでclientが申告するOpenID Connect application type。
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ApplicationType {
    Native,
    Web,
}

/// RFC 7591互換endpointで受け取る公開clientのmetadata。
#[derive(Deserialize)]
pub struct DynamicClientRegistrationRequest {
    pub client_name: Option<String>,
    pub redirect_uris: Option<Vec<String>>,
    pub token_endpoint_auth_method: Option<String>,
    pub grant_types: Option<Vec<String>>,
    pub response_types: Option<Vec<String>>,
    pub application_type: Option<ApplicationType>,
}

/// DCRで登録した公開clientのmetadata。
#[derive(Serialize)]
pub struct DynamicClientRegistrationResponse {
    pub client_id: String,
    pub client_name: String,
    pub redirect_uris: Vec<String>,
    pub token_endpoint_auth_method: &'static str,
    pub grant_types: [&'static str; 2],
    pub response_types: [&'static str; 1],
    #[serde(skip_serializing_if = "Option::is_none")]
    pub application_type: Option<ApplicationType>,
}

impl DynamicClientRegistrationResponse {
    pub fn new(client: Client, application_type: Option<ApplicationType>) -> Self {
        Self {
            client_id: client.client_id,
            client_name: client.display_name,
            redirect_uris: client.redirect_uris,
            token_endpoint_auth_method: "none",
            grant_types: ["authorization_code", "refresh_token"],
            response_types: ["code"],
            application_type,
        }
    }
}

pub struct ValidatedDynamicClientRegistration {
    pub client: Client,
    pub application_type: Option<ApplicationType>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DynamicClientRegistrationError {
    MissingRedirectUris,
    UnsupportedMetadata,
}

/// 登録情報との照合を終えたredirect URIと、認可要求での指定有無。
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ResolvedRedirectUri {
    Supplied(String),
    Inferred(String),
}

impl ResolvedRedirectUri {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Supplied(value) | Self::Inferred(value) => value,
        }
    }

    pub fn was_supplied(&self) -> bool {
        matches!(self, Self::Supplied(_))
    }
}

/// 認可codeとtokenへ保存する、利用者、client、resource、scopeの結合。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthorizationGrant {
    pub principal: Principal,
    pub client_id: String,
    pub redirect_uri: ResolvedRedirectUri,
    pub resource_uri: String,
    pub scopes: Vec<String>,
}

/// 有効なaccess tokenから復元した主体とscope。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthenticatedPrincipal {
    pub principal: Principal,
    pub scopes: Vec<String>,
}

/// Unix epochからのミリ秒。保存adapterとの時刻の受け渡しに使う。
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Timestamp(i64);

impl Timestamp {
    pub const fn new(milliseconds: i64) -> Self {
        Self(milliseconds)
    }

    pub const fn get(self) -> i64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuthorizationError {
    InvalidRequest,
    InvalidClient,
    InvalidRedirectUri,
    InvalidScope,
    InvalidTarget,
    InvalidGrant,
    Capacity,
    Unavailable,
}

/// Authorization Code Flowでtransportから受け取る未検証の認可要求。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthorizationRequest {
    pub client_id: String,
    pub redirect_uri: Option<String>,
    pub resource_uri: String,
    pub scopes: Vec<String>,
    pub code_challenge: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthorizationClient {
    pub client: Client,
    pub registration_method: ClientRegistrationMethod,
    pub redirect_uri: String,
}

/// Authorization Serverがclient情報を得た方式。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClientRegistrationMethod {
    Dynamic,
    MetadataDocument,
}

/// 永続化したclientと、その情報を更新する際に使う登録方式。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RegisteredClient {
    pub client: Client,
    pub registration_method: ClientRegistrationMethod,
}

/// client登録とredirect URIを照合し、既定scopeも解決した認可要求。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidatedAuthorizationRequest {
    pub client: Client,
    pub registration_method: ClientRegistrationMethod,
    pub redirect_uri: ResolvedRedirectUri,
    pub resource_uri: String,
    pub scopes: Vec<String>,
    pub code_challenge: String,
}

/// token endpointだけが短時間保持するtoken pair。秘密値のためDebugを実装しない。
pub struct TokenPair {
    pub access_token: String,
    pub refresh_token: String,
    pub access_expires_in_seconds: u64,
    pub scope: String,
}

/// refresh token rotationでrepositoryへ渡す、生成済みの新旧tokenとbinding。
pub struct RefreshTokenRotation {
    pub refresh_token: String,
    pub client_id: String,
    pub resource_uri: String,
    pub requested_scopes: Option<Vec<String>>,
    pub new_access_token: String,
    pub new_refresh_token: String,
    pub access_expires_at: Timestamp,
    pub refresh_expires_at: Timestamp,
}

/// 認可codeの一回消費とtoken pair発行を同じtransactionで行うための入力。
pub struct AuthorizationCodeExchange {
    pub code: String,
    pub client_id: String,
    pub redirect_uri: Option<String>,
    pub resource_uri: String,
    pub code_challenge: String,
    pub access_token: String,
    pub refresh_token: String,
    pub access_expires_at: Timestamp,
    pub refresh_expires_at: Timestamp,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RefreshTokenRotationOutcome {
    Rotated { access_scopes: Vec<String> },
    InvalidToken,
    InvalidScope,
}
