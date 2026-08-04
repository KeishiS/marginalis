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
