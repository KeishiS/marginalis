//! サーバーの設定境界。環境変数とNixOS moduleはこの型へ変換される。

use core::fmt;
use std::{env, net::SocketAddr, path::PathBuf, sync::Arc};

use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use marginalis_application::{
    AuthenticationUseCaseError, Clock, McpAuthorizationRequest, McpOAuthUseCaseError,
    McpOAuthUseCases, McpRefreshTokenRotation, McpTokenPair, NoteUseCaseError, NoteUseCases,
    OidcAuthenticationUseCases, Random, SessionLifetime, WebSessionUseCases,
};
use marginalis_auth_oidc::{OidcAuthentication, OidcCallbackError, OidcConfiguration};
use marginalis_domain::{
    Actor, AuthenticatedSession, EntityId, Note, NoteDraft, NoteId, NotePermission, UnixMillis,
    WebSession,
};
use marginalis_sqlite::{SqliteDatabase, SqliteStoreError};
use sha2::{Digest, Sha256};
use url::Url;
use uuid::Uuid;

/// server組立時に使うUTC millisecond clock。
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> UnixMillis {
        UnixMillis::new(time::OffsetDateTime::now_utc().unix_timestamp_nanos() as i64 / 1_000_000)
    }
}

/// UUIDv7と暗号学的に安全な不透明tokenを生成する実行環境adapter。
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemRandom;

impl Random for SystemRandom {
    fn uuid_v7(&self) -> EntityId {
        EntityId::from_uuid_v7(Uuid::now_v7())
    }

    fn opaque_token(&self) -> String {
        let bytes: [u8; 32] = rand::random();
        URL_SAFE_NO_PAD.encode(bytes)
    }
}

/// adapter群を組み合わせて、transportへノート操作だけを公開するserver側実装。
#[derive(Clone, Debug)]
pub struct ServerNoteUseCases {
    database: SqliteDatabase,
}

impl ServerNoteUseCases {
    pub fn new(database: SqliteDatabase) -> Self {
        Self { database }
    }
}

/// OIDC login時に検証したgroup claimをsessionへ固定するv0.3 Cookie session service。
#[derive(Clone)]
pub struct ServerWebSessionUseCases {
    database: SqliteDatabase,
    lifetime: SessionLifetime,
}

/// v0.3 loginではKanidm group以外の利用者状態を保存しない。
#[derive(Clone)]
pub struct ServerOidcAuthenticationUseCases {
    database: SqliteDatabase,
    configuration: OidcConfiguration,
    http_client: reqwest::Client,
    oidc: Arc<tokio::sync::RwLock<Option<OidcAuthentication>>>,
}

impl ServerOidcAuthenticationUseCases {
    pub fn new(
        database: SqliteDatabase,
        configuration: OidcConfiguration,
        http_client: reqwest::Client,
        oidc: Option<OidcAuthentication>,
    ) -> Self {
        Self {
            database,
            configuration,
            http_client,
            oidc: Arc::new(tokio::sync::RwLock::new(oidc)),
        }
    }

    /// Discovery失敗後も次のログイン要求で再試行する。service再起動をIdP復旧の前提にしない。
    async fn oidc(&self) -> Result<OidcAuthentication, AuthenticationUseCaseError> {
        if let Some(oidc) = self.oidc.read().await.clone() {
            return Ok(oidc);
        }
        let discovered = OidcAuthentication::discover_with_http_client(
            &self.configuration,
            self.http_client.clone(),
        )
        .await
        .map_err(|_| AuthenticationUseCaseError::Unavailable)?;
        let mut oidc = self.oidc.write().await;
        Ok(oidc.get_or_insert(discovered).clone())
    }
}

impl ServerWebSessionUseCases {
    pub fn new(database: SqliteDatabase, lifetime: SessionLifetime) -> Self {
        Self { database, lifetime }
    }
}

/// OAuth code exchangeの成功時だけtransportへ返すtoken pair。Debugを実装しない。
pub struct McpIssuedTokenPair {
    pub access_token: String,
    pub refresh_token: String,
    pub access_expires_in_seconds: u64,
    pub scope: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum McpOAuthError {
    Rejected,
    Unavailable,
}

/// v0.3 SQLite schemaとKanidm主体を使うMCP OAuth service。
#[derive(Clone)]
pub struct ServerMcpOAuthService {
    database: SqliteDatabase,
    resource_uri: String,
}

impl ServerMcpOAuthService {
    pub const ACCESS_TOKEN_SECONDS: u64 = 60 * 60;
    pub const REFRESH_TOKEN_SECONDS: u64 = 30 * 24 * 60 * 60;
    const MAX_DYNAMIC_CLIENTS: i64 = 1_000;
    const UNUSED_CLIENT_SECONDS: i64 = 24 * 60 * 60;

    pub fn new(database: SqliteDatabase, resource_uri: String) -> Self {
        Self {
            database,
            resource_uri,
        }
    }

    pub async fn register_client(
        &self,
        client: marginalis_domain::McpOAuthClient,
    ) -> Result<(), McpOAuthError> {
        if client.client_id.is_empty()
            || client.display_name.trim().is_empty()
            || client.redirect_uris.is_empty()
            || client.display_name.len() > 128
            || client.redirect_uris.len() > 8
            || !client
                .redirect_uris
                .iter()
                .all(|uri| uri.len() <= 2_048 && valid_redirect_uri(uri))
        {
            return Err(McpOAuthError::Rejected);
        }
        let now = SystemClock.now();
        let registered = self
            .database
            .register_mcp_client_bounded(
                &client,
                now,
                UnixMillis::new(now.get() - Self::UNUSED_CLIENT_SECONDS * 1_000),
                Self::MAX_DYNAMIC_CLIENTS,
            )
            .await
            .map_err(|_| McpOAuthError::Unavailable)?;
        if !registered {
            return Err(McpOAuthError::Rejected);
        }
        Ok(())
    }

    pub async fn authorize(
        &self,
        actor: Actor,
        request: McpAuthorizationRequest,
    ) -> Result<String, McpOAuthError> {
        self.validate_authorization_request(&request).await?;
        let code = SystemRandom.opaque_token();
        let grant = marginalis_domain::McpAuthorizationGrant {
            actor,
            client_id: request.client_id,
            redirect_uri: request.redirect_uri,
            resource_uri: request.resource_uri,
            scopes: request.scopes,
        };
        self.database
            .issue_mcp_authorization_code(
                &code,
                &grant,
                &request.code_challenge,
                UnixMillis::new(SystemClock.now().get() + 5 * 60 * 1_000),
            )
            .await
            .map_err(|_| McpOAuthError::Unavailable)?;
        Ok(code)
    }

    pub async fn validate_authorization_request(
        &self,
        request: &McpAuthorizationRequest,
    ) -> Result<marginalis_domain::McpOAuthClient, McpOAuthError> {
        if request.resource_uri != self.resource_uri
            || !valid_mcp_scopes(&request.scopes)
            || !valid_pkce_challenge(&request.code_challenge)
            || !valid_redirect_uri(&request.redirect_uri)
        {
            return Err(McpOAuthError::Rejected);
        }
        let Some(client) = self
            .database
            .mcp_client(&request.client_id)
            .await
            .map_err(|_| McpOAuthError::Unavailable)?
        else {
            return Err(McpOAuthError::Rejected);
        };
        if !client.redirect_uris.contains(&request.redirect_uri) {
            return Err(McpOAuthError::Rejected);
        }
        Ok(client)
    }

    pub async fn exchange_authorization_code(
        &self,
        code: String,
        client_id: String,
        redirect_uri: String,
        resource_uri: String,
        verifier: String,
    ) -> Result<McpIssuedTokenPair, McpOAuthError> {
        if resource_uri != self.resource_uri || !valid_pkce_verifier(&verifier) {
            return Err(McpOAuthError::Rejected);
        }
        let expected_challenge = pkce_s256(&verifier);
        let now = SystemClock.now();
        let Some(grant) = self
            .database
            .consume_mcp_authorization_code(
                &code,
                &client_id,
                &redirect_uri,
                &resource_uri,
                &expected_challenge,
                now,
            )
            .await
            .map_err(|_| McpOAuthError::Unavailable)?
        else {
            return Err(McpOAuthError::Rejected);
        };
        self.issue_pair(grant, now).await
    }

    pub async fn refresh_access_token(
        &self,
        refresh_token: String,
        client_id: String,
        resource_uri: String,
    ) -> Result<McpIssuedTokenPair, McpOAuthError> {
        if resource_uri != self.resource_uri {
            return Err(McpOAuthError::Rejected);
        }
        let now = SystemClock.now();
        let access_token = SystemRandom.opaque_token();
        let next_refresh_token = SystemRandom.opaque_token();
        let Some(grant) = self
            .database
            .rotate_mcp_refresh_token(
                McpRefreshTokenRotation {
                    refresh_token,
                    client_id,
                    resource_uri,
                    new_access_token: access_token.clone(),
                    new_refresh_token: next_refresh_token.clone(),
                    access_expires_at: UnixMillis::new(
                        now.get() + (Self::ACCESS_TOKEN_SECONDS * 1_000) as i64,
                    ),
                    refresh_expires_at: UnixMillis::new(
                        now.get() + (Self::REFRESH_TOKEN_SECONDS * 1_000) as i64,
                    ),
                },
                now,
            )
            .await
            .map_err(|_| McpOAuthError::Unavailable)?
        else {
            return Err(McpOAuthError::Rejected);
        };
        Ok(McpIssuedTokenPair {
            access_token,
            refresh_token: next_refresh_token,
            access_expires_in_seconds: Self::ACCESS_TOKEN_SECONDS,
            scope: grant.scopes.join(" "),
        })
    }

    async fn issue_pair(
        &self,
        grant: marginalis_domain::McpAuthorizationGrant,
        now: UnixMillis,
    ) -> Result<McpIssuedTokenPair, McpOAuthError> {
        let access_token = SystemRandom.opaque_token();
        let refresh_token = SystemRandom.opaque_token();
        self.database
            .issue_mcp_token_pair(
                &access_token,
                &refresh_token,
                &grant,
                UnixMillis::new(now.get() + (Self::ACCESS_TOKEN_SECONDS * 1_000) as i64),
                UnixMillis::new(now.get() + (Self::REFRESH_TOKEN_SECONDS * 1_000) as i64),
                now,
            )
            .await
            .map_err(|_| McpOAuthError::Unavailable)?;
        Ok(McpIssuedTokenPair {
            access_token,
            refresh_token,
            access_expires_in_seconds: Self::ACCESS_TOKEN_SECONDS,
            scope: grant.scopes.join(" "),
        })
    }

    pub async fn authenticate(
        &self,
        token: &str,
        resource_uri: &str,
        scope: &str,
    ) -> Result<Option<marginalis_domain::McpAuthenticatedActor>, McpOAuthError> {
        let Some(authenticated) = self
            .database
            .authenticate_mcp_access_token(token, resource_uri, scope, SystemClock.now())
            .await
            .map_err(|_| McpOAuthError::Unavailable)?
        else {
            return Ok(None);
        };
        Ok(Some(authenticated))
    }

    pub async fn revoke(&self, actor: &Actor, client_id: &str) -> Result<(), McpOAuthError> {
        self.database
            .revoke_mcp_client_tokens(&actor.issuer, &actor.subject, client_id, SystemClock.now())
            .await
            .map_err(|_| McpOAuthError::Unavailable)
    }
}

fn mcp_error(error: McpOAuthError) -> McpOAuthUseCaseError {
    match error {
        McpOAuthError::Rejected => McpOAuthUseCaseError::Rejected,
        McpOAuthError::Unavailable => McpOAuthUseCaseError::Unavailable,
    }
}

#[async_trait]
impl McpOAuthUseCases for ServerMcpOAuthService {
    async fn register_client(
        &self,
        client: marginalis_domain::McpOAuthClient,
    ) -> Result<(), McpOAuthUseCaseError> {
        self.register_client(client).await.map_err(mcp_error)
    }
    async fn validate_authorization_request(
        &self,
        request: McpAuthorizationRequest,
    ) -> Result<marginalis_domain::McpOAuthClient, McpOAuthUseCaseError> {
        self.validate_authorization_request(&request)
            .await
            .map_err(mcp_error)
    }
    async fn authorize(
        &self,
        actor: Actor,
        request: McpAuthorizationRequest,
    ) -> Result<String, McpOAuthUseCaseError> {
        self.authorize(actor, request).await.map_err(mcp_error)
    }
    async fn exchange_authorization_code(
        &self,
        code: String,
        client_id: String,
        redirect_uri: String,
        resource_uri: String,
        verifier: String,
    ) -> Result<McpTokenPair, McpOAuthUseCaseError> {
        let pair = self
            .exchange_authorization_code(code, client_id, redirect_uri, resource_uri, verifier)
            .await
            .map_err(mcp_error)?;
        Ok(McpTokenPair {
            access_token: pair.access_token,
            refresh_token: pair.refresh_token,
            access_expires_in_seconds: pair.access_expires_in_seconds,
            scope: pair.scope,
        })
    }
    async fn refresh_access_token(
        &self,
        refresh_token: String,
        client_id: String,
        resource_uri: String,
    ) -> Result<McpTokenPair, McpOAuthUseCaseError> {
        let pair = self
            .refresh_access_token(refresh_token, client_id, resource_uri)
            .await
            .map_err(mcp_error)?;
        Ok(McpTokenPair {
            access_token: pair.access_token,
            refresh_token: pair.refresh_token,
            access_expires_in_seconds: pair.access_expires_in_seconds,
            scope: pair.scope,
        })
    }
    async fn authenticate(
        &self,
        token: String,
        resource_uri: String,
        scope: String,
    ) -> Result<Option<marginalis_domain::McpAuthenticatedActor>, McpOAuthUseCaseError> {
        self.authenticate(&token, &resource_uri, &scope)
            .await
            .map_err(mcp_error)
    }
    async fn revoke(&self, actor: Actor, client_id: String) -> Result<(), McpOAuthUseCaseError> {
        self.revoke(&actor, &client_id).await.map_err(mcp_error)
    }
}

fn pkce_s256(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

fn valid_pkce_challenge(value: &str) -> bool {
    value.len() == 43
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn valid_pkce_verifier(value: &str) -> bool {
    (43..=128).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~'))
}

fn valid_mcp_scopes(scopes: &[String]) -> bool {
    !scopes.is_empty()
        && scopes.iter().all(|scope| {
            matches!(
                scope.as_str(),
                "notes:read" | "notes:write" | "notes:delete"
            )
        })
}

fn valid_redirect_uri(value: &str) -> bool {
    let Ok(url) = Url::parse(value) else {
        return false;
    };
    if url.host().is_none()
        || url.query().is_some()
        || url.fragment().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return false;
    }
    url.scheme() == "https"
        || (url.scheme() == "http"
            && matches!(
                url.host(),
                Some(url::Host::Ipv4(address)) if address.is_loopback()
            ))
        || (url.scheme() == "http"
            && matches!(
                url.host(),
                Some(url::Host::Ipv6(address)) if address.is_loopback()
            ))
}

fn map_note_error(error: SqliteStoreError) -> NoteUseCaseError {
    match error {
        SqliteStoreError::Conflict | SqliteStoreError::LastAdmin => NoteUseCaseError::Conflict,
        SqliteStoreError::CorruptNote | SqliteStoreError::ArchiveFormat => {
            NoteUseCaseError::Validation
        }
        SqliteStoreError::ArchiveTargetNotEmpty
        | SqliteStoreError::ArchiveMissingAdmin
        | SqliteStoreError::Database(_) => NoteUseCaseError::Unavailable,
    }
}

#[async_trait]
impl NoteUseCases for ServerNoteUseCases {
    async fn list_visible_notes(&self, actor: Actor) -> Result<Vec<Note>, NoteUseCaseError> {
        self.database
            .list_visible_notes(&actor, 0, 1_000)
            .await
            .map_err(map_note_error)
    }

    async fn read_note(&self, actor: Actor, note_id: NoteId) -> Result<Note, NoteUseCaseError> {
        self.database
            .visible_note(&actor, note_id, NotePermission::Read)
            .await
            .map_err(map_note_error)?
            .ok_or(NoteUseCaseError::NotFound)
    }

    async fn create_note(&self, actor: Actor, draft: NoteDraft) -> Result<Note, NoteUseCaseError> {
        let draft = marginalis_asciidoc::validate_note_draft(draft)
            .map_err(|_| NoteUseCaseError::Validation)?;
        let now = SystemClock.now();
        let note = Note {
            note_id: NoteId::new(SystemRandom.uuid_v7()),
            creator_issuer: actor.issuer.clone(),
            creator_subject: actor.subject.clone(),
            title: draft.title,
            body: draft.body,
            tags: draft.tags,
            created_at: now,
            updated_at: now,
            revision: 1,
            deleted_at: None,
        };
        self.database
            .create_note(&note, NotePermission::Admin)
            .await
            .map_err(map_note_error)?;
        Ok(note)
    }

    async fn update_note(
        &self,
        actor: Actor,
        note_id: NoteId,
        draft: NoteDraft,
        expected_revision: i64,
    ) -> Result<Note, NoteUseCaseError> {
        self.database
            .visible_note(&actor, note_id, NotePermission::Write)
            .await
            .map_err(map_note_error)?
            .ok_or(NoteUseCaseError::NotFound)?;
        let draft = marginalis_asciidoc::validate_note_draft(draft)
            .map_err(|_| NoteUseCaseError::Validation)?;
        self.database
            .update_note(note_id, expected_revision, &draft, SystemClock.now())
            .await
            .map_err(map_note_error)
    }

    async fn soft_delete_note(
        &self,
        actor: Actor,
        note_id: NoteId,
        expected_revision: i64,
    ) -> Result<Note, NoteUseCaseError> {
        self.database
            .visible_note(&actor, note_id, NotePermission::Admin)
            .await
            .map_err(map_note_error)?
            .ok_or(NoteUseCaseError::NotFound)?;
        self.database
            .soft_delete_note(note_id, expected_revision, SystemClock.now())
            .await
            .map_err(map_note_error)?;
        self.database
            .note(note_id, true)
            .await
            .map_err(map_note_error)?
            .ok_or(NoteUseCaseError::Unavailable)
    }

    async fn restore_note(
        &self,
        actor: Actor,
        note_id: NoteId,
        expected_revision: i64,
    ) -> Result<Note, NoteUseCaseError> {
        self.database
            .visible_deleted_note(&actor, note_id)
            .await
            .map_err(map_note_error)?
            .ok_or(NoteUseCaseError::NotFound)?;
        self.database
            .restore_note(note_id, expected_revision, SystemClock.now())
            .await
            .map_err(map_note_error)
    }

    fn export_note_source(&self, note: &Note) -> Result<String, NoteUseCaseError> {
        marginalis_asciidoc::export_note(note).map_err(|_| NoteUseCaseError::Unavailable)
    }

    fn render_note_html(&self, note: &Note) -> Result<String, NoteUseCaseError> {
        marginalis_asciidoc::render_note_html(note).map_err(|_| NoteUseCaseError::Validation)
    }
}

#[async_trait]
impl WebSessionUseCases for ServerWebSessionUseCases {
    async fn authenticate_session(
        &self,
        session_id: String,
    ) -> Result<Option<AuthenticatedSession>, AuthenticationUseCaseError> {
        let now = SystemClock.now();
        let Some(session) = self
            .database
            .lookup_web_session(&session_id, now)
            .await
            .map_err(|_| AuthenticationUseCaseError::Unavailable)?
        else {
            return Ok(None);
        };
        Ok(Some(session))
    }

    async fn verify_csrf(
        &self,
        session_id: String,
        csrf_token: String,
    ) -> Result<bool, AuthenticationUseCaseError> {
        self.database
            .validate_web_session_csrf(&session_id, &csrf_token)
            .await
            .map_err(|_| AuthenticationUseCaseError::Unavailable)
    }

    async fn issue_session(&self, actor: Actor) -> Result<WebSession, AuthenticationUseCaseError> {
        let now = SystemClock.now();
        let session = WebSession {
            session_id: SystemRandom.opaque_token(),
            csrf_token: SystemRandom.opaque_token(),
            actor,
            idle_expires_at: UnixMillis::new(now.get() + self.lifetime.idle_timeout_ms),
            absolute_expires_at: UnixMillis::new(now.get() + self.lifetime.absolute_timeout_ms),
        };
        self.database
            .issue_web_session(&session, now)
            .await
            .map_err(|_| AuthenticationUseCaseError::Unavailable)?;
        Ok(session)
    }

    async fn revoke_session(&self, session_id: String) -> Result<(), AuthenticationUseCaseError> {
        self.database
            .revoke_web_session(&session_id, SystemClock.now())
            .await
            .map_err(|_| AuthenticationUseCaseError::Unavailable)
    }
}

#[async_trait]
impl OidcAuthenticationUseCases for ServerOidcAuthenticationUseCases {
    async fn begin_login(&self) -> Result<String, AuthenticationUseCaseError> {
        self.oidc()
            .await?
            .begin_login(
                &self.database.oidc_login_attempt_store(),
                &SystemRandom,
                &SystemClock,
            )
            .await
            .map_err(|_| AuthenticationUseCaseError::Unavailable)
    }

    async fn complete_login(
        &self,
        code: String,
        state: String,
    ) -> Result<Actor, AuthenticationUseCaseError> {
        let identity = self
            .oidc()
            .await?
            .complete_login(
                &self.database.oidc_login_attempt_store(),
                &SystemClock,
                &code,
                &state,
                "groups",
            )
            .await
            .map_err(|error| match error {
                OidcCallbackError::Rejected(_) => AuthenticationUseCaseError::Rejected,
                OidcCallbackError::Unavailable => AuthenticationUseCaseError::Unavailable,
            })?;
        if !identity.groups.is_user("server-users") {
            return Err(AuthenticationUseCaseError::Rejected);
        }
        Ok(Actor {
            issuer: identity.issuer,
            subject: identity.subject,
            is_administrator: identity.groups.is_administrator("server-admins"),
        })
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServerConfig {
    pub http: HttpConfig,
    pub storage: StorageConfig,
    pub oidc: OidcConfig,
    pub mcp_enabled: bool,
    pub mcp_allowed_origins: Vec<String>,
}

/// HTTP transportだけが必要とする公開設定。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HttpConfig {
    pub base_url: Url,
    pub listen_address: SocketAddr,
}

/// SQLiteとAsciiDoc正本だけを扱うmaintenance command向けの設定境界。
///
/// backupおよびprojection再構築はHTTP listener・OIDC client・secretを必要としないため、
/// `ServerConfig`を読まずこの型だけを利用する。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StorageConfig {
    pub database_url: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OidcConfig {
    pub issuer_url: Url,
    pub client_id: String,
    pub ca_certificate_file: Option<PathBuf>,
}

/// secret値は公開設定から分離する。Debugを実装せずログ出力を防ぐ。
pub struct SecretConfig {
    pub oidc_client_secret: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConfigurationError {
    MissingEnvironment(&'static str),
    InvalidBaseUrl,
    InvalidIssuerUrl,
    InvalidListenAddress,
    EmptyClientId,
    UnreadableSecretFile(&'static str),
    InvalidMcpEnable,
    InvalidMcpAllowedOrigin,
}

impl fmt::Display for ConfigurationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingEnvironment(name) => {
                write!(formatter, "required environment variable {name} is not set")
            }
            Self::InvalidBaseUrl => formatter.write_str(
                "MARGINALIS_BASE_URL must be an absolute HTTPS URL without query or fragment",
            ),
            Self::InvalidIssuerUrl => {
                formatter.write_str("OIDC_ISSUER_URL must be an absolute HTTPS URL")
            }
            Self::InvalidListenAddress => formatter.write_str("MARGINALIS_LISTEN_ADDR is invalid"),
            Self::EmptyClientId => formatter.write_str("OIDC_CLIENT_ID must not be empty"),
            Self::UnreadableSecretFile(name) => {
                write!(formatter, "secret file for {name} could not be read")
            }
            Self::InvalidMcpEnable => {
                formatter.write_str("MARGINALIS_MCP_ENABLE must be `true` or `false`")
            }
            Self::InvalidMcpAllowedOrigin => formatter.write_str(
                "MARGINALIS_MCP_ALLOWED_ORIGINS must contain comma-separated HTTPS origins",
            ),
        }
    }
}

impl std::error::Error for ConfigurationError {}

impl ServerConfig {
    pub fn from_environment() -> Result<(Self, SecretConfig), ConfigurationError> {
        let base_url = validate_base_url(required("MARGINALIS_BASE_URL")?)?;
        let issuer_url = validate_issuer_url(required("OIDC_ISSUER_URL")?)?;
        let client_id = required("OIDC_CLIENT_ID")?;
        if client_id.is_empty() {
            return Err(ConfigurationError::EmptyClientId);
        }
        let storage = StorageConfig::from_environment()?;
        let listen_address = required("MARGINALIS_LISTEN_ADDR")?
            .parse()
            .map_err(|_| ConfigurationError::InvalidListenAddress)?;
        let configuration = Self {
            http: HttpConfig {
                base_url,
                listen_address,
            },
            storage,
            oidc: OidcConfig {
                issuer_url,
                client_id,
                ca_certificate_file: std::env::var_os("OIDC_CA_CERTIFICATE_FILE")
                    .filter(|value| !value.is_empty())
                    .map(PathBuf::from),
            },
            mcp_enabled: optional_bool("MARGINALIS_MCP_ENABLE")?.unwrap_or(false),
            mcp_allowed_origins: validate_mcp_allowed_origins(optional_csv(
                "MARGINALIS_MCP_ALLOWED_ORIGINS",
            )?)?,
        };
        let secrets = SecretConfig {
            oidc_client_secret: required_secret("OIDC_CLIENT_SECRET")?,
        };
        Ok((configuration, secrets))
    }
}

impl StorageConfig {
    pub fn from_environment() -> Result<Self, ConfigurationError> {
        Ok(Self {
            database_url: required("MARGINALIS_DATABASE_URL")?,
        })
    }
}

fn optional_bool(name: &'static str) -> Result<Option<bool>, ConfigurationError> {
    match env::var(name) {
        Ok(value) => match value.as_str() {
            "true" => Ok(Some(true)),
            "false" => Ok(Some(false)),
            _ => Err(ConfigurationError::InvalidMcpEnable),
        },
        Err(env::VarError::NotPresent) => Ok(None),
        Err(env::VarError::NotUnicode(_)) => Err(ConfigurationError::InvalidMcpEnable),
    }
}

fn optional_csv(name: &'static str) -> Result<Vec<String>, ConfigurationError> {
    match env::var(name) {
        Ok(value) => Ok(value
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .collect()),
        Err(env::VarError::NotPresent) => Ok(Vec::new()),
        Err(env::VarError::NotUnicode(_)) => Err(ConfigurationError::InvalidMcpEnable),
    }
}

fn validate_mcp_allowed_origins(values: Vec<String>) -> Result<Vec<String>, ConfigurationError> {
    let mut origins = Vec::with_capacity(values.len());
    for value in values {
        let url = Url::parse(&value).map_err(|_| ConfigurationError::InvalidMcpAllowedOrigin)?;
        if url.scheme() != "https"
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.path() != "/"
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(ConfigurationError::InvalidMcpAllowedOrigin);
        }
        let origin = url.origin().ascii_serialization();
        if !origins.contains(&origin) {
            origins.push(origin);
        }
    }
    Ok(origins)
}

fn required_secret(name: &'static str) -> Result<String, ConfigurationError> {
    optional_secret(name)?.ok_or(ConfigurationError::MissingEnvironment(name))
}

fn optional_secret(name: &'static str) -> Result<Option<String>, ConfigurationError> {
    let file_variable = format!("{name}_FILE");
    if let Some(path) = env::var_os(file_variable) {
        let value = std::fs::read_to_string(path)
            .map_err(|_| ConfigurationError::UnreadableSecretFile(name))?
            .trim_end_matches(['\r', '\n'])
            .to_owned();
        return (!value.is_empty())
            .then_some(value)
            .ok_or(ConfigurationError::MissingEnvironment(name))
            .map(Some);
    }
    Ok(env::var(name).ok().filter(|value| !value.is_empty()))
}

fn required(name: &'static str) -> Result<String, ConfigurationError> {
    env::var(name)
        .ok()
        .filter(|value| !value.is_empty())
        .ok_or(ConfigurationError::MissingEnvironment(name))
}

fn validate_base_url(value: String) -> Result<Url, ConfigurationError> {
    let url = Url::parse(&value).map_err(|_| ConfigurationError::InvalidBaseUrl)?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(ConfigurationError::InvalidBaseUrl);
    }
    Ok(url)
}

fn validate_issuer_url(value: String) -> Result<Url, ConfigurationError> {
    let url = Url::parse(&value).map_err(|_| ConfigurationError::InvalidIssuerUrl)?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(ConfigurationError::InvalidIssuerUrl);
    }
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_url_rejects_non_https() {
        assert_eq!(
            validate_base_url("http://example.test".into()),
            Err(ConfigurationError::InvalidBaseUrl)
        );
    }

    #[tokio::test]
    async fn oidc_unavailability_rejects_login_without_preventing_service_construction() {
        let database = SqliteDatabase::connect("sqlite::memory:")
            .await
            .expect("database");
        let configuration = OidcConfiguration::new(
            "https://127.0.0.1:1".into(),
            "marginalis".into(),
            "test-secret".into(),
            "https://marginalis.example.test",
        )
        .expect("configuration");
        let authentication = ServerOidcAuthenticationUseCases::new(
            database,
            configuration,
            reqwest::Client::new(),
            None,
        );
        assert_eq!(
            authentication.begin_login().await,
            Err(AuthenticationUseCaseError::Unavailable)
        );
    }

    #[test]
    fn base_url_accepts_subpath() {
        assert_eq!(
            validate_base_url("https://example.test/marginalis".into())
                .expect("valid URL")
                .path(),
            "/marginalis"
        );
    }

    #[test]
    fn mcp_allowed_origins_are_normalized_and_reject_non_origins() {
        assert_eq!(
            validate_mcp_allowed_origins(vec![
                "https://chatgpt.com".into(),
                "https://chatgpt.com".into(),
            ])
            .expect("origins"),
            vec!["https://chatgpt.com"]
        );
        for invalid in [
            "http://chatgpt.com",
            "https://chatgpt.com/path",
            "https://user@chatgpt.com",
            "not-an-origin",
        ] {
            assert_eq!(
                validate_mcp_allowed_origins(vec![invalid.into()]),
                Err(ConfigurationError::InvalidMcpAllowedOrigin)
            );
        }
    }

    #[test]
    fn oauth_redirects_require_https_or_an_ip_loopback() {
        for valid in [
            "https://chatgpt.com/connector/oauth/callback",
            "http://127.0.0.1:48123/callback",
            "http://[::1]:48123/callback",
        ] {
            assert!(valid_redirect_uri(valid), "{valid}");
        }
        for invalid in [
            "http://localhost:48123/callback",
            "http://client.example.test/callback",
            "https://client.example.test/callback?next=other",
            "https://user@client.example.test/callback",
        ] {
            assert!(!valid_redirect_uri(invalid), "{invalid}");
        }
    }

    #[tokio::test]
    async fn notes_use_kanidm_subjects_and_sqlite_as_the_only_store() {
        let database = SqliteDatabase::connect("sqlite::memory:")
            .await
            .expect("database");
        let service = ServerNoteUseCases::new(database);
        let owner = Actor {
            issuer: "https://kanidm.example.test/oauth2/openid/marginalis".into(),
            subject: "owner".into(),
            is_administrator: false,
        };
        let reader = Actor {
            issuer: owner.issuer.clone(),
            subject: "reader".into(),
            is_administrator: false,
        };
        let note = service
            .create_note(
                owner.clone(),
                NoteDraft {
                    title: "SQLite canonical note".into(),
                    body: "Only SQLite persists this body.".into(),
                    tags: vec!["v3".into(), "sqlite".into()],
                },
            )
            .await
            .expect("create");
        assert_eq!(note.creator_subject, "owner");
        assert_eq!(
            service.read_note(reader, note.note_id).await,
            Err(NoteUseCaseError::NotFound)
        );
        let updated = service
            .update_note(
                owner.clone(),
                note.note_id,
                NoteDraft {
                    title: "Updated title".into(),
                    body: "Updated body".into(),
                    tags: vec!["sqlite".into()],
                },
                note.revision,
            )
            .await
            .expect("update");
        assert_eq!(updated.revision, note.revision + 1);
        let deleted = service
            .soft_delete_note(owner.clone(), note.note_id, updated.revision)
            .await
            .expect("soft delete");
        assert!(deleted.deleted_at.is_some());
        assert!(
            service
                .list_visible_notes(owner.clone())
                .await
                .expect("visible notes")
                .is_empty()
        );
        let restored = service
            .restore_note(owner, note.note_id, deleted.revision)
            .await
            .expect("restore");
        assert!(restored.deleted_at.is_none());
    }

    #[tokio::test]
    async fn session_retains_login_time_group_snapshot() {
        let database = SqliteDatabase::connect("sqlite::memory:")
            .await
            .expect("database");
        let now = SystemClock.now();
        let session = WebSession {
            session_id: "stale-session".into(),
            csrf_token: "csrf".into(),
            actor: Actor {
                issuer: "https://kanidm.example.test".into(),
                subject: "removed-user".into(),
                is_administrator: false,
            },
            idle_expires_at: UnixMillis::new(now.get() + 60_000),
            absolute_expires_at: UnixMillis::new(now.get() + 60_000),
        };
        database
            .issue_web_session(&session, now)
            .await
            .expect("issue");
        let service = ServerWebSessionUseCases::new(
            database.clone(),
            SessionLifetime {
                idle_timeout_ms: 60_000,
                absolute_timeout_ms: 60_000,
            },
        );
        assert_eq!(
            service
                .authenticate_session(session.session_id.clone())
                .await
                .expect("snapshot"),
            Some(AuthenticatedSession {
                actor: session.actor,
                idle_expires_at: session.idle_expires_at,
                absolute_expires_at: session.absolute_expires_at,
            })
        );
    }

    #[tokio::test]
    async fn mcp_oauth_rotates_tokens_and_honors_revocation() {
        let database = SqliteDatabase::connect("sqlite::memory:")
            .await
            .expect("database");
        let resource_uri = "https://notes.example.test/mcp".to_owned();
        let service = ServerMcpOAuthService::new(database, resource_uri.clone());
        let client = marginalis_domain::McpOAuthClient {
            client_id: "https://client.example.test/mcp.json".into(),
            display_name: "Client".into(),
            redirect_uris: vec!["https://client.example.test/callback".into()],
        };
        service
            .register_client(client.clone())
            .await
            .expect("client");
        let actor = Actor {
            issuer: "https://id.example.test".into(),
            subject: "alice".into(),
            is_administrator: false,
        };
        let verifier = "v3-pkce-verifier-which-is-at-least-forty-three-characters".to_owned();
        assert_eq!(
            service
                .validate_authorization_request(&McpAuthorizationRequest {
                    client_id: client.client_id.clone(),
                    redirect_uri: client.redirect_uris[0].clone(),
                    resource_uri: "https://other.example.test/mcp".into(),
                    scopes: vec!["notes:read".into()],
                    code_challenge: pkce_s256(&verifier),
                })
                .await,
            Err(McpOAuthError::Rejected)
        );
        assert_eq!(
            service
                .validate_authorization_request(&McpAuthorizationRequest {
                    client_id: client.client_id.clone(),
                    redirect_uri: client.redirect_uris[0].clone(),
                    resource_uri: resource_uri.clone(),
                    scopes: vec!["notes:read".into()],
                    code_challenge: "short".into(),
                })
                .await,
            Err(McpOAuthError::Rejected)
        );
        let code = service
            .authorize(
                actor.clone(),
                McpAuthorizationRequest {
                    client_id: client.client_id.clone(),
                    redirect_uri: client.redirect_uris[0].clone(),
                    resource_uri: resource_uri.clone(),
                    scopes: vec!["notes:read".into()],
                    code_challenge: pkce_s256(&verifier),
                },
            )
            .await
            .expect("authorize");
        let tokens = service
            .exchange_authorization_code(
                code,
                client.client_id.clone(),
                client.redirect_uris[0].clone(),
                resource_uri.clone(),
                verifier.clone(),
            )
            .await
            .expect("exchange");
        assert!(
            service
                .authenticate(&tokens.access_token, &resource_uri, "notes:read")
                .await
                .expect("authenticate")
                .is_some()
        );
        let original_refresh_token = tokens.refresh_token.clone();
        let rotated = service
            .refresh_access_token(
                tokens.refresh_token,
                client.client_id.clone(),
                resource_uri.clone(),
            )
            .await
            .expect("refresh");
        assert!(matches!(
            service
                .refresh_access_token(
                    original_refresh_token,
                    client.client_id.clone(),
                    resource_uri.clone(),
                )
                .await,
            Err(McpOAuthError::Rejected)
        ));
        assert!(
            service
                .authenticate(&rotated.access_token, &resource_uri, "notes:read")
                .await
                .expect("replayed token family")
                .is_none()
        );

        let replacement_code = service
            .authorize(
                actor.clone(),
                McpAuthorizationRequest {
                    client_id: client.client_id.clone(),
                    redirect_uri: client.redirect_uris[0].clone(),
                    resource_uri: resource_uri.clone(),
                    scopes: vec!["notes:read".into()],
                    code_challenge: pkce_s256(&verifier),
                },
            )
            .await
            .expect("replacement authorization");
        let replacement = service
            .exchange_authorization_code(
                replacement_code,
                client.client_id.clone(),
                client.redirect_uris[0].clone(),
                resource_uri.clone(),
                verifier,
            )
            .await
            .expect("replacement exchange");
        service
            .revoke(&actor, &client.client_id)
            .await
            .expect("revoke");
        assert!(
            service
                .authenticate(&replacement.access_token, &resource_uri, "notes:read")
                .await
                .expect("revoked")
                .is_none()
        );
    }
}
