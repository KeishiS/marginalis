//! サーバーの設定境界。環境変数とNixOS moduleはこの型へ変換される。

use core::fmt;
use std::{env, net::SocketAddr, path::PathBuf, str::FromStr, sync::Arc, time::Duration};

use adocweave::attributes::AttributeOperation;
use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use marginalis_application::{
    AuthenticationUseCaseError, Clock, DeleteConfirmation, DeleteConfirmationStore,
    DeletePreparation, McpAccessTokenStore, McpAuthorizationRequest,
    McpOAuthAdministrationUseCases, McpOAuthStore, McpOAuthUseCaseError, McpOAuthUseCases,
    McpRefreshTokenRotation, McpTokenPair, NoteAclService, NoteAclServiceError, NoteAclStore,
    NoteDraft, NoteOperationKind, NoteQueryStore, NoteUseCaseError, NoteUseCases, NoteWriteService,
    OidcAuthenticationUseCases, OidcUserAdministrationStore, Random, RootCredentialStore,
    SessionLifetime, UserAdministrationUseCases, WebSession, WebSessionService, WebSessionStore,
    WebSessionUseCases,
};
use marginalis_auth_oidc::{OidcAuthentication, OidcCallbackError};
use marginalis_domain::{
    Actor, EntityId, McpAuthorizationGrant, NoteId, NotePage, NotePermission, NoteSource,
    OidcLoginResult, RegistrationPolicy, RootAuditAction, RootAuditEvent, SourceRevision,
    UnixMillis, UserId,
};
use marginalis_files::FileNoteStore;
use marginalis_mcp::{McpAuthenticationError, McpAuthenticator};
use marginalis_sqlite::SqliteDatabase;
use serde::Deserialize;
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
    sources: FileNoteStore,
    /// 正本revisionの照合からrename/deleteまでを一つの臨界区間にする。
    /// SQLiteとfilesystemをまたぐため、初期版ではprocess内の全ノート書込みを直列化する。
    write_lock: Arc<tokio::sync::Mutex<()>>,
}

const OIDC_SESSION_IDLE_TIMEOUT_MS: i64 = 24 * 60 * 60 * 1_000;
const OIDC_SESSION_ABSOLUTE_TIMEOUT_MS: i64 = 7 * 24 * 60 * 60 * 1_000;
const ROOT_SESSION_IDLE_TIMEOUT_MS: i64 = 30 * 60 * 1_000;
const ROOT_SESSION_ABSOLUTE_TIMEOUT_MS: i64 = 8 * 60 * 60 * 1_000;

const fn oidc_session_lifetime() -> SessionLifetime {
    SessionLifetime {
        idle_timeout_ms: OIDC_SESSION_IDLE_TIMEOUT_MS,
        absolute_timeout_ms: OIDC_SESSION_ABSOLUTE_TIMEOUT_MS,
    }
}

const fn root_session_lifetime() -> SessionLifetime {
    SessionLifetime {
        idle_timeout_ms: ROOT_SESSION_IDLE_TIMEOUT_MS,
        absolute_timeout_ms: ROOT_SESSION_ABSOLUTE_TIMEOUT_MS,
    }
}

/// Web session、外部OIDCとroot管理を同じapplication境界で公開するserver adapter。
#[derive(Clone)]
pub struct ServerWebAuthenticationUseCases {
    database: SqliteDatabase,
    oidc: Option<OidcAuthentication>,
    cookie_path: String,
}

/// MCP bearer tokenのresource audienceとscopeを検証するserver adapter。
#[derive(Clone)]
pub struct ServerMcpAuthenticator {
    database: SqliteDatabase,
    resource_uri: String,
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

#[derive(Deserialize)]
struct ClientMetadataDocument {
    client_id: String,
    client_name: String,
    redirect_uris: Vec<String>,
}

/// MCP OAuthのcode発行・exchangeをSQLite adapterへ接続するapplication service。
#[derive(Clone)]
pub struct ServerMcpOAuthService {
    database: SqliteDatabase,
    metadata_client: reqwest::Client,
    metadata_allowed_hosts: Vec<String>,
}

impl ServerMcpOAuthService {
    pub fn new(database: SqliteDatabase, metadata_allowed_hosts: Vec<String>) -> Self {
        Self {
            database,
            metadata_client: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(5))
                .build()
                .expect("static reqwest configuration is valid"),
            metadata_allowed_hosts,
        }
    }

    async fn lookup_or_fetch_client(
        &self,
        client_id: String,
    ) -> Result<Option<marginalis_domain::McpOAuthClient>, McpOAuthError> {
        if let Some(client) = self
            .database
            .mcp_oauth_store()
            .lookup_client(client_id.clone())
            .await
            .map_err(|_| McpOAuthError::Unavailable)?
        {
            return Ok(Some(client));
        }
        let Ok(client_url) = Url::parse(&client_id) else {
            return Ok(None);
        };
        if client_url.scheme() != "https"
            || client_url.query().is_some()
            || client_url.fragment().is_some()
            || !client_url.username().is_empty()
            || client_url.password().is_some()
            || !client_url.host_str().is_some_and(|host| {
                self.metadata_allowed_hosts
                    .iter()
                    .any(|allowed| allowed == host)
            })
        {
            return Ok(None);
        }
        let response = self
            .metadata_client
            .get(client_url)
            .send()
            .await
            .map_err(|_| McpOAuthError::Unavailable)?;
        if !response.status().is_success()
            || response
                .content_length()
                .is_some_and(|length| length > 65_536)
        {
            return Ok(None);
        }
        let body = response
            .bytes()
            .await
            .map_err(|_| McpOAuthError::Unavailable)?;
        if body.len() > 65_536 {
            return Ok(None);
        }
        let Ok(metadata) = serde_json::from_slice::<ClientMetadataDocument>(&body) else {
            return Ok(None);
        };
        if metadata.client_id != client_id
            || metadata.client_name.trim().is_empty()
            || metadata.redirect_uris.is_empty()
            || !metadata
                .redirect_uris
                .iter()
                .all(|uri| valid_redirect_uri(uri))
        {
            return Ok(None);
        }
        let client = marginalis_domain::McpOAuthClient {
            client_id,
            display_name: metadata.client_name,
            redirect_uris: metadata.redirect_uris,
        };
        self.database
            .mcp_oauth_store()
            .upsert_client(client.clone())
            .await
            .map_err(|_| McpOAuthError::Unavailable)?;
        Ok(Some(client))
    }

    pub async fn authorize(
        &self,
        grant: McpAuthorizationGrant,
        code_challenge: String,
    ) -> Result<String, McpOAuthError> {
        let Some(client) = self.lookup_or_fetch_client(grant.client_id.clone()).await? else {
            return Err(McpOAuthError::Rejected);
        };
        if !client.redirect_uris.contains(&grant.redirect_uri) || code_challenge.is_empty() {
            return Err(McpOAuthError::Rejected);
        }
        let code = SystemRandom.opaque_token();
        self.database
            .mcp_oauth_store()
            .issue_authorization_code(
                code.clone(),
                grant,
                code_challenge,
                UnixMillis::new(SystemClock.now().get() + 5 * 60 * 1_000),
            )
            .await
            .map_err(|_| McpOAuthError::Unavailable)?;
        Ok(code)
    }

    pub async fn exchange_authorization_code(
        &self,
        code: String,
        client_id: String,
        redirect_uri: String,
        resource_uri: String,
        code_verifier: String,
    ) -> Result<McpIssuedTokenPair, McpOAuthError> {
        let now = SystemClock.now();
        let Some((grant, expected_challenge)) = self
            .database
            .mcp_oauth_store()
            .consume_authorization_code(code, client_id, redirect_uri, resource_uri, now)
            .await
            .map_err(|_| McpOAuthError::Unavailable)?
        else {
            return Err(McpOAuthError::Rejected);
        };
        if pkce_s256(&code_verifier) != expected_challenge {
            return Err(McpOAuthError::Rejected);
        }
        let access_token = SystemRandom.opaque_token();
        let refresh_token = SystemRandom.opaque_token();
        let access_expires_in_seconds = 60 * 60;
        let scope = grant.scopes.join(" ");
        self.database
            .mcp_oauth_store()
            .issue_token_pair(
                access_token.clone(),
                refresh_token.clone(),
                grant,
                UnixMillis::new(now.get() + (access_expires_in_seconds * 1_000) as i64),
                UnixMillis::new(now.get() + 30 * 24 * 60 * 60 * 1_000),
                now,
            )
            .await
            .map_err(|_| McpOAuthError::Unavailable)?;
        Ok(McpIssuedTokenPair {
            access_token,
            refresh_token,
            access_expires_in_seconds,
            scope,
        })
    }

    pub async fn refresh_access_token(
        &self,
        refresh_token: String,
        client_id: String,
        resource_uri: String,
    ) -> Result<McpIssuedTokenPair, McpOAuthError> {
        let now = SystemClock.now();
        let access_token = SystemRandom.opaque_token();
        let next_refresh_token = SystemRandom.opaque_token();
        let access_expires_in_seconds = 60 * 60;
        let grant = self
            .database
            .mcp_oauth_store()
            .rotate_refresh_token(
                McpRefreshTokenRotation {
                    refresh_token,
                    client_id,
                    resource_uri,
                    new_access_token: access_token.clone(),
                    new_refresh_token: next_refresh_token.clone(),
                    access_expires_at: UnixMillis::new(
                        now.get() + (access_expires_in_seconds * 1_000) as i64,
                    ),
                    refresh_expires_at: UnixMillis::new(now.get() + 30 * 24 * 60 * 60 * 1_000),
                },
                now,
            )
            .await
            .map_err(|_| McpOAuthError::Unavailable)?;
        let Some(grant) = grant else {
            return Err(McpOAuthError::Rejected);
        };
        Ok(McpIssuedTokenPair {
            access_token,
            refresh_token: next_refresh_token,
            access_expires_in_seconds,
            scope: grant.scopes.join(" "),
        })
    }
}

fn pkce_s256(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
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
    if url.query().is_some()
        || url.fragment().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return false;
    }
    url.scheme() == "https"
        || (url.scheme() == "http"
            && matches!(
                url.host_str(),
                Some("127.0.0.1") | Some("localhost") | Some("::1")
            ))
}

#[async_trait]
impl McpOAuthUseCases for ServerMcpOAuthService {
    async fn validate_authorization_request(
        &self,
        request: McpAuthorizationRequest,
    ) -> Result<marginalis_domain::McpOAuthClient, McpOAuthUseCaseError> {
        if request.code_challenge.is_empty()
            || request.resource_uri.trim().is_empty()
            || !valid_mcp_scopes(&request.scopes)
            || !valid_redirect_uri(&request.redirect_uri)
        {
            return Err(McpOAuthUseCaseError::Rejected);
        }
        let client = self
            .lookup_or_fetch_client(request.client_id)
            .await
            .map_err(|error| match error {
                McpOAuthError::Rejected => McpOAuthUseCaseError::Rejected,
                McpOAuthError::Unavailable => McpOAuthUseCaseError::Unavailable,
            })?
            .ok_or(McpOAuthUseCaseError::Rejected)?;
        if !client.redirect_uris.contains(&request.redirect_uri) {
            return Err(McpOAuthUseCaseError::Rejected);
        }
        Ok(client)
    }

    async fn authorize(
        &self,
        actor: Actor,
        request: McpAuthorizationRequest,
    ) -> Result<String, McpOAuthUseCaseError> {
        if actor.is_root {
            return Err(McpOAuthUseCaseError::Rejected);
        }
        self.validate_authorization_request(request.clone()).await?;
        ServerMcpOAuthService::authorize(
            self,
            McpAuthorizationGrant {
                user_id: actor.user_id,
                client_id: request.client_id,
                redirect_uri: request.redirect_uri,
                resource_uri: request.resource_uri,
                scopes: request.scopes,
            },
            request.code_challenge,
        )
        .await
        .map_err(|error| match error {
            McpOAuthError::Rejected => McpOAuthUseCaseError::Rejected,
            McpOAuthError::Unavailable => McpOAuthUseCaseError::Unavailable,
        })
    }

    async fn exchange_authorization_code(
        &self,
        code: String,
        client_id: String,
        redirect_uri: String,
        resource_uri: String,
        code_verifier: String,
    ) -> Result<McpTokenPair, McpOAuthUseCaseError> {
        let tokens = ServerMcpOAuthService::exchange_authorization_code(
            self,
            code,
            client_id,
            redirect_uri,
            resource_uri,
            code_verifier,
        )
        .await
        .map_err(|error| match error {
            McpOAuthError::Rejected => McpOAuthUseCaseError::Rejected,
            McpOAuthError::Unavailable => McpOAuthUseCaseError::Unavailable,
        })?;
        Ok(McpTokenPair {
            access_token: tokens.access_token,
            refresh_token: tokens.refresh_token,
            access_expires_in_seconds: tokens.access_expires_in_seconds,
            scope: tokens.scope,
        })
    }

    async fn refresh_access_token(
        &self,
        refresh_token: String,
        client_id: String,
        resource_uri: String,
    ) -> Result<McpTokenPair, McpOAuthUseCaseError> {
        let tokens = ServerMcpOAuthService::refresh_access_token(
            self,
            refresh_token,
            client_id,
            resource_uri,
        )
        .await
        .map_err(|error| match error {
            McpOAuthError::Rejected => McpOAuthUseCaseError::Rejected,
            McpOAuthError::Unavailable => McpOAuthUseCaseError::Unavailable,
        })?;
        Ok(McpTokenPair {
            access_token: tokens.access_token,
            refresh_token: tokens.refresh_token,
            access_expires_in_seconds: tokens.access_expires_in_seconds,
            scope: tokens.scope,
        })
    }
}

#[async_trait]
impl McpOAuthAdministrationUseCases for ServerMcpOAuthService {
    async fn register_client(
        &self,
        actor: Actor,
        client: marginalis_domain::McpOAuthClient,
    ) -> Result<(), McpOAuthUseCaseError> {
        if !actor.is_root
            || client.client_id.trim().is_empty()
            || client.display_name.trim().is_empty()
            || client.redirect_uris.is_empty()
            || !client
                .redirect_uris
                .iter()
                .all(|uri| valid_redirect_uri(uri))
        {
            return Err(McpOAuthUseCaseError::Rejected);
        }
        let client_id = client.client_id.clone();
        self.database
            .mcp_oauth_store()
            .upsert_client(client)
            .await
            .map_err(|_| McpOAuthUseCaseError::Unavailable)?;
        self.database
            .record_root_audit(RootAuditEvent {
                action: RootAuditAction::McpClientRegistered,
                actor_user_id: Some(actor.user_id),
                target_user_id: None,
                target: Some(client_id),
                occurred_at: SystemClock.now(),
            })
            .await
            .map_err(|_| McpOAuthUseCaseError::Unavailable)
    }

    async fn revoke_client_authorization(
        &self,
        actor: Actor,
        user_id: UserId,
        client_id: String,
    ) -> Result<(), McpOAuthUseCaseError> {
        if !actor.is_root && actor.user_id != user_id {
            return Err(McpOAuthUseCaseError::Rejected);
        }
        self.database
            .mcp_oauth_store()
            .revoke_client_tokens(user_id, client_id.clone(), SystemClock.now())
            .await
            .map_err(|_| McpOAuthUseCaseError::Unavailable)?;
        if actor.is_root {
            self.database
                .record_root_audit(RootAuditEvent {
                    action: RootAuditAction::McpClientAuthorizationRevoked,
                    actor_user_id: Some(actor.user_id),
                    target_user_id: Some(user_id),
                    target: Some(client_id),
                    occurred_at: SystemClock.now(),
                })
                .await
                .map_err(|_| McpOAuthUseCaseError::Unavailable)?;
        }
        Ok(())
    }

    async fn list_client_authorizations(
        &self,
        actor: Actor,
        user_id: marginalis_domain::UserId,
    ) -> Result<Vec<marginalis_domain::McpClientAuthorization>, McpOAuthUseCaseError> {
        if !actor.is_root && actor.user_id != user_id {
            return Err(McpOAuthUseCaseError::Rejected);
        }
        self.database
            .mcp_oauth_store()
            .list_client_authorizations(user_id)
            .await
            .map_err(|_| McpOAuthUseCaseError::Unavailable)
    }
}

impl ServerMcpAuthenticator {
    pub fn new(database: SqliteDatabase, resource_uri: String) -> Self {
        Self {
            database,
            resource_uri,
        }
    }
}

#[async_trait]
impl McpAuthenticator for ServerMcpAuthenticator {
    async fn authenticate(
        &self,
        bearer_token: &str,
        required_scope: &str,
    ) -> Result<Actor, McpAuthenticationError> {
        self.database
            .mcp_access_token_store()
            .authenticate(
                bearer_token.into(),
                self.resource_uri.clone(),
                required_scope.into(),
                SystemClock.now(),
            )
            .await
            .map_err(|_| McpAuthenticationError::Unavailable)?
            .ok_or(McpAuthenticationError::MissingOrInvalid)
    }
}

impl ServerWebAuthenticationUseCases {
    pub fn new(database: SqliteDatabase) -> Self {
        Self::with_cookie_path(database, "/".into())
    }

    pub fn with_cookie_path(database: SqliteDatabase, cookie_path: String) -> Self {
        Self {
            database,
            oidc: None,
            cookie_path,
        }
    }

    pub fn with_oidc(database: SqliteDatabase, oidc: OidcAuthentication) -> Self {
        Self::with_oidc_and_cookie_path(database, oidc.clone(), oidc.cookie_path().into())
    }

    pub fn with_oidc_and_cookie_path(
        database: SqliteDatabase,
        oidc: OidcAuthentication,
        cookie_path: String,
    ) -> Self {
        Self {
            database,
            oidc: Some(oidc),
            cookie_path,
        }
    }

    fn oidc(&self) -> Result<&OidcAuthentication, AuthenticationUseCaseError> {
        self.oidc
            .as_ref()
            .ok_or(AuthenticationUseCaseError::Unavailable)
    }
}

#[async_trait]
impl OidcAuthenticationUseCases for ServerWebAuthenticationUseCases {
    async fn begin_oidc_login(&self) -> Result<String, AuthenticationUseCaseError> {
        self.oidc()?
            .begin_login(
                &self.database.oidc_login_attempt_store(),
                &SystemRandom,
                &SystemClock,
            )
            .await
            .map_err(|_| AuthenticationUseCaseError::Unavailable)
    }

    async fn complete_oidc_login(
        &self,
        code: String,
        state: String,
    ) -> Result<OidcLoginResult, AuthenticationUseCaseError> {
        let registration_policy = self
            .database
            .registration_policy()
            .await
            .map_err(|_| AuthenticationUseCaseError::Unavailable)?;
        self.oidc()?
            .complete_login(
                &self.database.oidc_login_attempt_store(),
                &self.database.oidc_identity_store(),
                &SystemRandom,
                &SystemClock,
                registration_policy,
                &code,
                &state,
            )
            .await
            .map_err(|error| match error {
                OidcCallbackError::Rejected(_) => AuthenticationUseCaseError::Rejected,
                OidcCallbackError::Unavailable => AuthenticationUseCaseError::Unavailable,
            })
    }

    fn oidc_available(&self) -> bool {
        self.oidc.is_some()
    }

    fn cookie_path(&self) -> &str {
        &self.cookie_path
    }
}

#[async_trait]
impl WebSessionUseCases for ServerWebAuthenticationUseCases {
    async fn authenticate_session(
        &self,
        session_id: String,
    ) -> Result<Option<marginalis_application::AuthenticatedSession>, AuthenticationUseCaseError>
    {
        self.database
            .web_session_store()
            .lookup(session_id, SystemClock.now())
            .await
            .map_err(|_| AuthenticationUseCaseError::Unavailable)
    }

    async fn verify_csrf(
        &self,
        session_id: String,
        csrf_token: String,
    ) -> Result<bool, AuthenticationUseCaseError> {
        self.database
            .web_session_store()
            .verify_csrf(session_id, csrf_token, SystemClock.now())
            .await
            .map_err(|_| AuthenticationUseCaseError::Unavailable)
    }

    async fn issue_oidc_session(
        &self,
        user_id: UserId,
    ) -> Result<WebSession, AuthenticationUseCaseError> {
        WebSessionService::new(
            &self.database.web_session_store(),
            &SystemRandom,
            &SystemClock,
        )
        .issue(
            Actor {
                user_id,
                is_root: false,
            },
            oidc_session_lifetime(),
        )
        .await
        .map_err(|_| AuthenticationUseCaseError::Unavailable)
    }

    async fn root_login(
        &self,
        password: String,
    ) -> Result<Option<WebSession>, AuthenticationUseCaseError> {
        let Some(user_id) = self
            .database
            .root_credential_store()
            .verify_password(password)
            .await
            .map_err(|_| AuthenticationUseCaseError::Unavailable)?
        else {
            self.database
                .record_root_audit(RootAuditEvent {
                    action: RootAuditAction::LoginFailed,
                    actor_user_id: None,
                    target_user_id: None,
                    target: None,
                    occurred_at: SystemClock.now(),
                })
                .await
                .map_err(|_| AuthenticationUseCaseError::Unavailable)?;
            return Ok(None);
        };
        let session = WebSessionService::new(
            &self.database.web_session_store(),
            &SystemRandom,
            &SystemClock,
        )
        .issue(
            Actor {
                user_id,
                is_root: true,
            },
            root_session_lifetime(),
        )
        .await
        .map_err(|_| AuthenticationUseCaseError::Unavailable)?;
        self.database
            .record_root_audit(RootAuditEvent {
                action: RootAuditAction::LoginSucceeded,
                actor_user_id: Some(user_id),
                target_user_id: None,
                target: None,
                occurred_at: SystemClock.now(),
            })
            .await
            .map_err(|_| AuthenticationUseCaseError::Unavailable)?;
        Ok(Some(session))
    }

    async fn revoke_session(&self, session_id: String) -> Result<(), AuthenticationUseCaseError> {
        let now = SystemClock.now();
        let session = self
            .database
            .web_session_store()
            .lookup(session_id.clone(), now)
            .await
            .map_err(|_| AuthenticationUseCaseError::Unavailable)?;
        self.database
            .web_session_store()
            .revoke(session_id, now)
            .await
            .map_err(|_| AuthenticationUseCaseError::Unavailable)?;
        if let Some(session) = session.filter(|session| session.actor.is_root) {
            self.database
                .record_root_audit(RootAuditEvent {
                    action: RootAuditAction::Logout,
                    actor_user_id: Some(session.actor.user_id),
                    target_user_id: None,
                    target: None,
                    occurred_at: now,
                })
                .await
                .map_err(|_| AuthenticationUseCaseError::Unavailable)?;
        }
        Ok(())
    }
}

#[async_trait]
impl UserAdministrationUseCases for ServerWebAuthenticationUseCases {
    async fn list_pending_users(
        &self,
    ) -> Result<Vec<marginalis_domain::OidcUser>, AuthenticationUseCaseError> {
        self.database
            .oidc_user_administration_store()
            .list_pending()
            .await
            .map_err(|_| AuthenticationUseCaseError::Unavailable)
    }

    async fn activate_pending_user(
        &self,
        actor: Actor,
        user_id: UserId,
    ) -> Result<bool, AuthenticationUseCaseError> {
        if !actor.is_root {
            return Err(AuthenticationUseCaseError::Rejected);
        }
        let now = SystemClock.now();
        let activated = self
            .database
            .oidc_user_administration_store()
            .activate(user_id, now)
            .await
            .map_err(|_| AuthenticationUseCaseError::Unavailable)?;
        if activated {
            self.database
                .record_root_audit(RootAuditEvent {
                    action: RootAuditAction::OidcUserActivated,
                    actor_user_id: Some(actor.user_id),
                    target_user_id: Some(user_id),
                    target: None,
                    occurred_at: now,
                })
                .await
                .map_err(|_| AuthenticationUseCaseError::Unavailable)?;
        }
        Ok(activated)
    }

    async fn disable_oidc_user(
        &self,
        actor: Actor,
        user_id: UserId,
    ) -> Result<bool, AuthenticationUseCaseError> {
        if !actor.is_root {
            return Err(AuthenticationUseCaseError::Rejected);
        }
        let now = SystemClock.now();
        let disabled = self
            .database
            .oidc_user_administration_store()
            .disable(user_id, now)
            .await
            .map_err(|_| AuthenticationUseCaseError::Unavailable)?;
        if disabled {
            self.database
                .record_root_audit(RootAuditEvent {
                    action: RootAuditAction::OidcUserDisabled,
                    actor_user_id: Some(actor.user_id),
                    target_user_id: Some(user_id),
                    target: None,
                    occurred_at: now,
                })
                .await
                .map_err(|_| AuthenticationUseCaseError::Unavailable)?;
        }
        Ok(disabled)
    }
    async fn registration_policy(&self) -> Result<RegistrationPolicy, AuthenticationUseCaseError> {
        self.database
            .registration_policy()
            .await
            .map_err(|_| AuthenticationUseCaseError::Unavailable)
    }
    async fn set_registration_policy(
        &self,
        actor: Actor,
        policy: RegistrationPolicy,
    ) -> Result<(), AuthenticationUseCaseError> {
        if !actor.is_root {
            return Err(AuthenticationUseCaseError::Rejected);
        }
        self.database
            .set_registration_policy(policy)
            .await
            .map_err(|_| AuthenticationUseCaseError::Unavailable)?;
        self.database
            .record_root_audit(RootAuditEvent {
                action: RootAuditAction::RegistrationPolicyChanged,
                actor_user_id: Some(actor.user_id),
                target_user_id: None,
                target: Some(
                    match policy {
                        RegistrationPolicy::Open => "open",
                        RegistrationPolicy::Approval => "approval",
                        RegistrationPolicy::InviteOnly => "invite-only",
                    }
                    .into(),
                ),
                occurred_at: SystemClock.now(),
            })
            .await
            .map_err(|_| AuthenticationUseCaseError::Unavailable)
    }
}

impl ServerNoteUseCases {
    pub fn new(database: SqliteDatabase, sources: FileNoteStore) -> Self {
        Self {
            database,
            sources,
            write_lock: Arc::new(tokio::sync::Mutex::new(())),
        }
    }

    pub async fn recover(&self) -> Result<(), NoteUseCaseError> {
        let _write_guard = self.write_lock.lock().await;
        let projections = self.database.note_projection_store();
        let journal = self.database.operation_journal();
        NoteWriteService::new(
            &self.sources,
            &projections,
            &journal,
            &SystemRandom,
            &SystemClock,
        )
        .recover()
        .await
        .map_err(|_| NoteUseCaseError::Unavailable)
    }

    /// AsciiDoc正本を全件検証してから、SQLite検索・参照投影を一つのtransactionで置換する。
    ///
    /// 途中でUTF-8・profile・ファイル名と`note-id`の不一致があればDBへ一切書き込まない。
    pub async fn rebuild_projections(&self) -> Result<usize, NoteUseCaseError> {
        let _write_guard = self.write_lock.lock().await;
        let sources = self
            .sources
            .list_sources()
            .map_err(|_| NoteUseCaseError::Unavailable)?;
        let mut projections = Vec::with_capacity(sources.len());
        for (path_note_id, source) in sources {
            let source = std::str::from_utf8(&source).map_err(|_| NoteUseCaseError::Validation)?;
            let projection = marginalis_asciidoc::parse_note_projection(source)
                .map_err(|_| NoteUseCaseError::Validation)?;
            if projection.note_id != path_note_id {
                return Err(NoteUseCaseError::Validation);
            }
            projections.push((projection, SourceRevision::from_source(source.as_bytes())));
        }
        self.database
            .replace_all_note_projections(&projections)
            .await
            .map_err(|_| NoteUseCaseError::Unavailable)?;
        Ok(projections.len())
    }
}

fn timestamp_rfc3339(now: UnixMillis) -> Result<String, NoteUseCaseError> {
    let datetime =
        time::OffsetDateTime::from_unix_timestamp_nanos(i128::from(now.get()) * 1_000_000)
            .map_err(|_| NoteUseCaseError::Unavailable)?;
    let seconds = datetime
        .format(time::macros::format_description!(
            "[year]-[month]-[day]T[hour]:[minute]:[second]"
        ))
        .map_err(|_| NoteUseCaseError::Unavailable)?;
    Ok(format!("{seconds}.{:03}Z", now.get().rem_euclid(1_000)))
}

fn render_note_source(
    note_id: NoteId,
    creator_id: UserId,
    created_at: &str,
    updated_at: &str,
    draft: &NoteDraft,
) -> Result<String, NoteUseCaseError> {
    if draft.title.trim().is_empty()
        || draft.title.contains(['\r', '\n'])
        || draft.tags.iter().any(|tag| tag.contains(['\r', '\n']))
    {
        return Err(NoteUseCaseError::Validation);
    }
    Ok(format!(
        "= {}\n:note-id: {}\n:creator-id: {}\n:created-at: {}\n:updated-at: {}\n:tags: {}\n\n{}\n",
        draft.title,
        note_id,
        creator_id,
        created_at,
        updated_at,
        draft.tags.join(", "),
        draft.body,
    ))
}

fn source_metadata(source: &str) -> Result<marginalis_asciidoc::NoteMetadata, NoteUseCaseError> {
    let analysis = adocweave::Engine::new(Default::default())
        .analyze(source)
        .map_err(|_| NoteUseCaseError::Unavailable)?;
    marginalis_asciidoc::validate_note_metadata(&analysis)
        .map_err(|_| NoteUseCaseError::Unavailable)
}

/// raw AsciiDoc APIの保護属性を、解析済みのattribute rangeだけでサーバ値へ置換する。
///
/// header全体を再生成しないため、利用者が書いた他のAsciiDoc属性と文書構造を保持する。
fn replace_protected_attributes(
    source: String,
    replacements: &[(&str, &str)],
) -> Result<String, NoteUseCaseError> {
    let analysis = adocweave::Engine::new(Default::default())
        .analyze(&source)
        .map_err(|_| NoteUseCaseError::Validation)?;
    let mut ranges = Vec::with_capacity(replacements.len());
    for (name, value) in replacements {
        let attributes = analysis
            .ast()
            .attributes()
            .iter()
            .filter(|attribute| attribute.name == *name)
            .collect::<Vec<_>>();
        let Some(attribute) = attributes.first() else {
            return Err(NoteUseCaseError::Validation);
        };
        if attributes.len() != 1 || attribute.operation != AttributeOperation::Set {
            return Err(NoteUseCaseError::Validation);
        }
        let start = usize::try_from(attribute.value_range.start().to_u32())
            .map_err(|_| NoteUseCaseError::Validation)?;
        let end = usize::try_from(attribute.value_range.end().to_u32())
            .map_err(|_| NoteUseCaseError::Validation)?;
        ranges.push((start, end, *value));
    }
    ranges.sort_by_key(|(start, _, _)| std::cmp::Reverse(*start));
    let mut rewritten = source;
    for (start, end, value) in ranges {
        rewritten.replace_range(start..end, value);
    }
    Ok(rewritten)
}

#[async_trait]
impl NoteUseCases for ServerNoteUseCases {
    async fn list_notes(
        &self,
        actor: Actor,
        offset: u64,
        limit: u32,
    ) -> Result<NotePage, NoteUseCaseError> {
        self.database
            .note_query_store()
            .list_visible(actor, offset, limit)
            .await
            .map_err(|_| NoteUseCaseError::Unavailable)
    }

    async fn search_notes(
        &self,
        actor: Actor,
        query: String,
        offset: u64,
        limit: u32,
    ) -> Result<NotePage, NoteUseCaseError> {
        if query.trim().is_empty() {
            return Err(NoteUseCaseError::Validation);
        }
        self.database
            .note_query_store()
            .search_visible(actor, query, offset, limit)
            .await
            .map_err(|_| NoteUseCaseError::Unavailable)
    }

    async fn list_note_links(
        &self,
        actor: Actor,
        note_id: NoteId,
        offset: u64,
        limit: u32,
    ) -> Result<marginalis_domain::NoteLinkPage, NoteUseCaseError> {
        self.database
            .note_query_store()
            .list_visible_links(actor, note_id, offset, limit)
            .await
            .map_err(|_| NoteUseCaseError::Unavailable)
    }

    async fn read_source(
        &self,
        actor: Actor,
        note_id: NoteId,
    ) -> Result<NoteSource, NoteUseCaseError> {
        let permission = self
            .database
            .note_acl_store()
            .permission_for(actor, note_id)
            .await
            .map_err(|_| NoteUseCaseError::Unavailable)?;
        if !matches!(permission, Some(value) if value.permits(NotePermission::Read)) {
            return Err(NoteUseCaseError::NotFound);
        }
        let content = self
            .sources
            .read(note_id)
            .map_err(|_| NoteUseCaseError::Unavailable)?
            .ok_or(NoteUseCaseError::NotFound)?;
        let source = std::str::from_utf8(&content).map_err(|_| NoteUseCaseError::Unavailable)?;
        let metadata = source_metadata(source)?;
        Ok(NoteSource {
            note_id,
            title: metadata.title,
            tags: metadata.tags.into_iter().map(|tag| tag.display).collect(),
            created_at: metadata.created_at,
            updated_at: metadata.updated_at,
            revision: SourceRevision::from_source(&content),
            content,
        })
    }

    async fn create_source(
        &self,
        actor: Actor,
        source: String,
    ) -> Result<NoteId, NoteUseCaseError> {
        let _write_guard = self.write_lock.lock().await;
        // 入力headerの構造だけを検証し、identityと時刻はクライアント値を採用しない。
        let note_id = NoteId::new(SystemRandom.uuid_v7());
        let note_id_text = note_id.to_string();
        let creator_id_text = actor.user_id.to_string();
        let now = timestamp_rfc3339(SystemClock.now())?;
        let source = replace_protected_attributes(
            source,
            &[
                ("note-id", &note_id_text),
                ("creator-id", &creator_id_text),
                ("created-at", &now),
                ("updated-at", &now),
            ],
        )?;
        let projection = marginalis_asciidoc::parse_note_projection(&source)
            .map_err(|_| NoteUseCaseError::Validation)?;
        if projection.owner_id != actor.user_id {
            return Err(NoteUseCaseError::Forbidden);
        }
        let projections = self.database.note_projection_store();
        let journal = self.database.operation_journal();
        NoteWriteService::new(
            &self.sources,
            &projections,
            &journal,
            &SystemRandom,
            &SystemClock,
        )
        .replace(NoteOperationKind::Create, projection, source.into_bytes())
        .await
        .map_err(|_| NoteUseCaseError::Unavailable)?;
        Ok(note_id)
    }

    async fn create_note(
        &self,
        actor: Actor,
        draft: NoteDraft,
    ) -> Result<NoteSource, NoteUseCaseError> {
        let note_id = NoteId::new(SystemRandom.uuid_v7());
        let now = timestamp_rfc3339(SystemClock.now())?;
        let source = render_note_source(note_id, actor.user_id, &now, &now, &draft)?;
        let note_id = self.create_source(actor, source).await?;
        self.read_source(actor, note_id).await
    }

    async fn update_source(
        &self,
        actor: Actor,
        note_id: NoteId,
        source: String,
        expected_revision: SourceRevision,
    ) -> Result<(), NoteUseCaseError> {
        let _write_guard = self.write_lock.lock().await;
        let permission = self
            .database
            .note_acl_store()
            .permission_for(actor, note_id)
            .await
            .map_err(|_| NoteUseCaseError::Unavailable)?;
        if !matches!(permission, Some(value) if value.permits(NotePermission::Write)) {
            return Err(NoteUseCaseError::NotFound);
        }
        let previous_source = self
            .sources
            .read(note_id)
            .map_err(|_| NoteUseCaseError::Unavailable)?
            .ok_or(NoteUseCaseError::NotFound)?;
        if SourceRevision::from_source(&previous_source) != expected_revision {
            return Err(NoteUseCaseError::Conflict);
        }
        let previous_source =
            std::str::from_utf8(&previous_source).map_err(|_| NoteUseCaseError::Unavailable)?;
        let previous_metadata = source_metadata(previous_source)?;
        // updated-atはクライアント入力を信頼しない。置換してから形式を検証するため、
        // 任意の過去時刻や不正な時刻を送られてもサーバ時刻以外は保存されない。
        let updated_at = timestamp_rfc3339(SystemClock.now())?;
        let source = replace_protected_attributes(source, &[("updated-at", &updated_at)])?;
        let candidate_metadata =
            source_metadata(&source).map_err(|_| NoteUseCaseError::Validation)?;
        if candidate_metadata.note_id != previous_metadata.note_id
            || candidate_metadata.creator_id != previous_metadata.creator_id
            || candidate_metadata.created_at != previous_metadata.created_at
        {
            return Err(NoteUseCaseError::Validation);
        }
        let projection = marginalis_asciidoc::parse_note_projection(&source)
            .map_err(|_| NoteUseCaseError::Validation)?;
        let projections = self.database.note_projection_store();
        let journal = self.database.operation_journal();
        NoteWriteService::new(
            &self.sources,
            &projections,
            &journal,
            &SystemRandom,
            &SystemClock,
        )
        .replace(NoteOperationKind::Update, projection, source.into_bytes())
        .await
        .map(|_| ())
        .map_err(|_| NoteUseCaseError::Unavailable)
    }

    async fn update_note(
        &self,
        actor: Actor,
        note_id: NoteId,
        draft: NoteDraft,
        expected_revision: SourceRevision,
    ) -> Result<NoteSource, NoteUseCaseError> {
        let current = self.read_source(actor, note_id).await?;
        if current.revision != expected_revision {
            return Err(NoteUseCaseError::Conflict);
        }
        let source =
            std::str::from_utf8(&current.content).map_err(|_| NoteUseCaseError::Unavailable)?;
        let metadata = source_metadata(source)?;
        let creator_id = EntityId::from_str(&metadata.creator_id)
            .map(UserId::new)
            .map_err(|_| NoteUseCaseError::Unavailable)?;
        let updated_at = timestamp_rfc3339(SystemClock.now())?;
        let source = render_note_source(
            note_id,
            creator_id,
            &metadata.created_at,
            &updated_at,
            &draft,
        )?;
        self.update_source(actor, note_id, source, expected_revision)
            .await?;
        self.read_source(actor, note_id).await
    }

    async fn delete_note(
        &self,
        actor: Actor,
        note_id: NoteId,
        expected_revision: SourceRevision,
    ) -> Result<(), NoteUseCaseError> {
        let _write_guard = self.write_lock.lock().await;
        self.delete_note_locked(actor, note_id, expected_revision)
            .await
    }

    async fn prepare_delete_note(
        &self,
        actor: Actor,
        note_id: NoteId,
        expected_revision: SourceRevision,
    ) -> Result<DeletePreparation, NoteUseCaseError> {
        let _write_guard = self.write_lock.lock().await;
        let permission = self
            .database
            .note_acl_store()
            .permission_for(actor, note_id)
            .await
            .map_err(|_| NoteUseCaseError::Unavailable)?;
        if !matches!(permission, Some(value) if value.permits(NotePermission::Admin)) {
            return Err(NoteUseCaseError::NotFound);
        }
        let note = self.read_source(actor, note_id).await?;
        if note.revision != expected_revision {
            return Err(NoteUseCaseError::Conflict);
        }
        let token = SystemRandom.opaque_token();
        let incoming_reference_count = self
            .database
            .delete_confirmation_store()
            .issue(
                token.clone(),
                actor,
                note_id,
                expected_revision,
                UnixMillis::new(SystemClock.now().get() + 5 * 60 * 1_000),
            )
            .await
            .map_err(|_| NoteUseCaseError::Unavailable)?;
        Ok(DeletePreparation {
            note_id,
            title: note.title,
            revision: note.revision,
            incoming_reference_count,
            confirmation_token: token,
        })
    }

    async fn confirm_delete_note(
        &self,
        actor: Actor,
        confirmation_token: String,
    ) -> Result<(), NoteUseCaseError> {
        let _write_guard = self.write_lock.lock().await;
        match self
            .database
            .delete_confirmation_store()
            .consume(confirmation_token, actor, SystemClock.now())
            .await
            .map_err(|_| NoteUseCaseError::Unavailable)?
        {
            DeleteConfirmation::Confirmed {
                note_id,
                expected_revision,
            } => {
                self.delete_note_locked(actor, note_id, expected_revision)
                    .await
            }
            DeleteConfirmation::Missing => Err(NoteUseCaseError::NotFound),
            DeleteConfirmation::Stale => Err(NoteUseCaseError::Conflict),
        }
    }

    async fn set_permission(
        &self,
        actor: Actor,
        note_id: NoteId,
        user_id: UserId,
        permission: Option<NotePermission>,
    ) -> Result<(), NoteUseCaseError> {
        NoteAclService::new(&self.database.note_acl_store())
            .set_permission(actor, note_id, user_id, permission)
            .await
            .map_err(|error| match error {
                NoteAclServiceError::Forbidden => NoteUseCaseError::Forbidden,
                NoteAclServiceError::Store(_) => NoteUseCaseError::Conflict,
            })
    }
}

impl ServerNoteUseCases {
    /// 書込みmutexを取得済みの削除処理。
    async fn delete_note_locked(
        &self,
        actor: Actor,
        note_id: NoteId,
        expected_revision: SourceRevision,
    ) -> Result<(), NoteUseCaseError> {
        let permission = self
            .database
            .note_acl_store()
            .permission_for(actor, note_id)
            .await
            .map_err(|_| NoteUseCaseError::Unavailable)?;
        if !matches!(permission, Some(value) if value.permits(NotePermission::Admin)) {
            return Err(NoteUseCaseError::NotFound);
        }
        let source = self
            .sources
            .read(note_id)
            .map_err(|_| NoteUseCaseError::Unavailable)?
            .ok_or(NoteUseCaseError::NotFound)?;
        if SourceRevision::from_source(&source) != expected_revision {
            return Err(NoteUseCaseError::Conflict);
        }
        let projections = self.database.note_projection_store();
        let journal = self.database.operation_journal();
        NoteWriteService::new(
            &self.sources,
            &projections,
            &journal,
            &SystemRandom,
            &SystemClock,
        )
        .delete(note_id)
        .await
        .map_err(|_| NoteUseCaseError::Unavailable)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServerConfig {
    pub http: HttpConfig,
    pub storage: StorageConfig,
    pub oidc: OidcConfig,
    pub mcp_enabled: bool,
    pub mcp_client_metadata_allowed_hosts: Vec<String>,
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
    pub data_dir: PathBuf,
    pub database_url: String,
    pub initial_registration_policy: RegistrationPolicy,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OidcConfig {
    pub issuer_url: Url,
    pub client_id: String,
}

/// secret値は公開設定から分離する。Debugを実装せずログ出力を防ぐ。
pub struct SecretConfig {
    pub oidc_client_secret: String,
    pub initial_root_password: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConfigurationError {
    MissingEnvironment(&'static str),
    InvalidBaseUrl,
    InvalidIssuerUrl,
    InvalidListenAddress,
    EmptyClientId,
    EmptyDataDirectory,
    UnreadableSecretFile(&'static str),
    InvalidMcpEnable,
    InvalidInitialRegistrationPolicy,
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
            Self::EmptyDataDirectory => {
                formatter.write_str("MARGINALIS_DATA_DIR must not be empty")
            }
            Self::UnreadableSecretFile(name) => {
                write!(formatter, "secret file for {name} could not be read")
            }
            Self::InvalidMcpEnable => {
                formatter.write_str("MARGINALIS_MCP_ENABLE must be `true` or `false`")
            }
            Self::InvalidInitialRegistrationPolicy => formatter
                .write_str("MARGINALIS_INITIAL_REGISTRATION_POLICY must be `open` or `approval`"),
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
            },
            mcp_enabled: optional_bool("MARGINALIS_MCP_ENABLE")?.unwrap_or(false),
            mcp_client_metadata_allowed_hosts: optional_csv(
                "MARGINALIS_MCP_CLIENT_METADATA_ALLOWED_HOSTS",
            )?,
        };
        let secrets = SecretConfig {
            oidc_client_secret: required_secret("OIDC_CLIENT_SECRET")?,
            initial_root_password: optional_secret("ROOT_PASSWORD")?,
        };
        Ok((configuration, secrets))
    }
}

impl StorageConfig {
    pub fn from_environment() -> Result<Self, ConfigurationError> {
        let data_dir = PathBuf::from(required("MARGINALIS_DATA_DIR")?);
        if data_dir.as_os_str().is_empty() {
            return Err(ConfigurationError::EmptyDataDirectory);
        }
        Ok(Self {
            data_dir,
            database_url: required("MARGINALIS_DATABASE_URL")?,
            initial_registration_policy: optional_initial_registration_policy()?,
        })
    }
}

fn optional_initial_registration_policy() -> Result<RegistrationPolicy, ConfigurationError> {
    match env::var("MARGINALIS_INITIAL_REGISTRATION_POLICY") {
        Ok(value) => match value.as_str() {
            "open" => Ok(RegistrationPolicy::Open),
            "approval" | "" => Ok(RegistrationPolicy::Approval),
            _ => Err(ConfigurationError::InvalidInitialRegistrationPolicy),
        },
        Err(env::VarError::NotPresent) => Ok(RegistrationPolicy::Approval),
        Err(env::VarError::NotUnicode(_)) => {
            Err(ConfigurationError::InvalidInitialRegistrationPolicy)
        }
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
    use marginalis_application::{
        McpOAuthAdministrationUseCases, McpOAuthUseCases, NoteUseCases, OidcIdentityStore,
    };
    use marginalis_domain::{McpOAuthClient, OidcIdentity, RegistrationPolicy};
    use marginalis_mcp::McpAuthenticator;

    #[test]
    fn base_url_rejects_non_https() {
        assert_eq!(
            validate_base_url("http://example.test".into()),
            Err(ConfigurationError::InvalidBaseUrl)
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
    fn ordinary_oidc_sessions_idle_after_twenty_four_hours() {
        assert_eq!(
            oidc_session_lifetime().idle_timeout_ms,
            24 * 60 * 60 * 1_000
        );
        assert_eq!(root_session_lifetime().idle_timeout_ms, 30 * 60 * 1_000);
    }

    #[tokio::test]
    async fn root_only_authentication_keeps_the_configured_cookie_subpath() {
        let database = SqliteDatabase::connect("sqlite::memory:")
            .await
            .expect("database");
        let authentication =
            ServerWebAuthenticationUseCases::with_cookie_path(database, "/marginalis".into());
        assert_eq!(
            OidcAuthenticationUseCases::cookie_path(&authentication),
            "/marginalis"
        );
    }

    #[test]
    fn structured_note_source_keeps_server_metadata_out_of_client_draft() {
        let note_id =
            NoteId::new(EntityId::from_str("01800000-0000-7000-8000-000000000091").expect("note"));
        let user_id =
            UserId::new(EntityId::from_str("01800000-0000-7000-8000-000000000092").expect("user"));
        let source = render_note_source(
            note_id,
            user_id,
            "2026-07-23T00:00:00.000Z",
            "2026-07-23T00:00:01.000Z",
            &NoteDraft {
                title: "Structured note".into(),
                body: "Body".into(),
                tags: vec!["research".into()],
            },
        )
        .expect("source");
        let metadata = source_metadata(&source).expect("metadata");
        assert_eq!(metadata.note_id, note_id.to_string());
        assert_eq!(metadata.creator_id, user_id.to_string());
        assert_eq!(metadata.title, "Structured note");
    }

    #[tokio::test]
    async fn raw_source_creation_replaces_client_supplied_protected_metadata() {
        let directory =
            std::env::temp_dir().join(format!("marginalis-raw-create-{}", Uuid::now_v7()));
        let database = SqliteDatabase::connect("sqlite::memory:")
            .await
            .expect("database");
        let actor_id =
            UserId::new(EntityId::from_str("01800000-0000-7000-8000-000000000096").expect("actor"));
        database
            .oidc_identity_store()
            .register_or_lookup(
                OidcIdentity::new("https://id.example.test", "raw-create", "Raw creator")
                    .expect("identity"),
                RegistrationPolicy::Open,
                actor_id,
                UnixMillis::new(0),
            )
            .await
            .expect("actor");
        let supplied_note_id =
            NoteId::new(EntityId::from_str("01800000-0000-7000-8000-000000000097").expect("note"));
        let supplied_creator_id = UserId::new(
            EntityId::from_str("01800000-0000-7000-8000-000000000098").expect("creator"),
        );
        let source = render_note_source(
            supplied_note_id,
            supplied_creator_id,
            "2000-01-01T00:00:00.000Z",
            "2000-01-01T00:00:00.000Z",
            &NoteDraft {
                title: "Raw source".into(),
                body: "The body is retained verbatim.".into(),
                tags: vec!["research".into()],
            },
        )
        .expect("source");
        let service =
            ServerNoteUseCases::new(database, FileNoteStore::open(&directory).expect("sources"));
        let actor = Actor {
            user_id: actor_id,
            is_root: false,
        };
        let note_id = service.create_source(actor, source).await.expect("create");
        let stored = service.read_source(actor, note_id).await.expect("read");
        let metadata = source_metadata(std::str::from_utf8(&stored.content).expect("UTF-8"))
            .expect("metadata");
        assert_ne!(note_id, supplied_note_id);
        assert_eq!(metadata.note_id, note_id.to_string());
        assert_eq!(metadata.creator_id, actor_id.to_string());
        assert_ne!(metadata.created_at, "2000-01-01T00:00:00.000Z");
        assert_eq!(metadata.updated_at, metadata.created_at);
        std::fs::remove_dir_all(directory).expect("remove directory");
    }

    #[tokio::test]
    async fn raw_source_update_rejects_immutable_changes_and_overwrites_updated_at() {
        let directory =
            std::env::temp_dir().join(format!("marginalis-raw-update-{}", Uuid::now_v7()));
        let database = SqliteDatabase::connect("sqlite::memory:")
            .await
            .expect("database");
        let actor_id =
            UserId::new(EntityId::from_str("01800000-0000-7000-8000-000000000099").expect("actor"));
        database
            .oidc_identity_store()
            .register_or_lookup(
                OidcIdentity::new("https://id.example.test", "raw-update", "Raw editor")
                    .expect("identity"),
                RegistrationPolicy::Open,
                actor_id,
                UnixMillis::new(0),
            )
            .await
            .expect("actor");
        let service =
            ServerNoteUseCases::new(database, FileNoteStore::open(&directory).expect("sources"));
        let actor = Actor {
            user_id: actor_id,
            is_root: false,
        };
        let created = service
            .create_note(
                actor,
                NoteDraft {
                    title: "Raw update".into(),
                    body: "Original body".into(),
                    tags: vec![],
                },
            )
            .await
            .expect("create");
        let original = String::from_utf8(created.content).expect("UTF-8");
        let immutable_change =
            original.replace(":created-at: ", ":created-at: 2000-01-01T00:00:00.000Z #");
        assert_eq!(
            service
                .update_source(actor, created.note_id, immutable_change, created.revision)
                .await,
            Err(NoteUseCaseError::Validation)
        );

        let replacement = original
            .replace("Original body", "Revised body")
            .replace(":updated-at: ", ":updated-at: not-a-timestamp #");
        service
            .update_source(actor, created.note_id, replacement, created.revision)
            .await
            .expect("update");
        let stored = service
            .read_source(actor, created.note_id)
            .await
            .expect("read updated");
        let stored = String::from_utf8(stored.content).expect("UTF-8");
        assert!(stored.contains("Revised body"));
        assert!(!stored.contains("not-a-timestamp"));
        assert!(source_metadata(&stored).is_ok());
        std::fs::remove_dir_all(directory).expect("remove directory");
    }

    #[tokio::test]
    async fn mcp_oauth_code_pkce_refresh_and_bearer_authentication_form_one_flow() {
        let database = SqliteDatabase::connect("sqlite::memory:")
            .await
            .expect("database");
        let user_id =
            UserId::new(EntityId::from_str("01800000-0000-7000-8000-000000000093").expect("user"));
        database
            .oidc_identity_store()
            .register_or_lookup(
                OidcIdentity::new("https://id.example.test", "subject", "User").expect("identity"),
                RegistrationPolicy::Open,
                user_id,
                UnixMillis::new(0),
            )
            .await
            .expect("active user");
        let service = ServerMcpOAuthService::new(database.clone(), Vec::new());
        service
            .register_client(
                Actor {
                    user_id,
                    is_root: true,
                },
                McpOAuthClient {
                    client_id: "test-client".into(),
                    display_name: "Test client".into(),
                    redirect_uris: vec!["http://127.0.0.1:4567/callback".into()],
                },
            )
            .await
            .expect("register client");
        let verifier = "PKCE verifier";
        let request = McpAuthorizationRequest {
            client_id: "test-client".into(),
            redirect_uri: "http://127.0.0.1:4567/callback".into(),
            resource_uri: "https://example.test/mcp".into(),
            scopes: vec!["notes:read".into()],
            code_challenge: pkce_s256(verifier),
        };
        service
            .validate_authorization_request(request.clone())
            .await
            .expect("validate authorization request");
        let code = <ServerMcpOAuthService as McpOAuthUseCases>::authorize(
            &service,
            Actor {
                user_id,
                is_root: false,
            },
            request,
        )
        .await
        .expect("authorization code");
        let tokens = service
            .exchange_authorization_code(
                code,
                "test-client".into(),
                "http://127.0.0.1:4567/callback".into(),
                "https://example.test/mcp".into(),
                verifier.into(),
            )
            .await
            .expect("token exchange");
        let authenticator =
            ServerMcpAuthenticator::new(database.clone(), "https://example.test/mcp".into());
        assert_eq!(
            authenticator
                .authenticate(&tokens.access_token, "notes:read")
                .await
                .expect("access token")
                .user_id,
            user_id
        );
        let original_refresh_token = tokens.refresh_token.clone();
        let refreshed = service
            .refresh_access_token(
                tokens.refresh_token,
                "test-client".into(),
                "https://example.test/mcp".into(),
            )
            .await
            .expect("refresh token");
        assert!(matches!(
            service
                .refresh_access_token(
                    original_refresh_token,
                    "test-client".into(),
                    "https://example.test/mcp".into(),
                )
                .await,
            Err(McpOAuthError::Rejected)
        ));
        assert_eq!(
            authenticator
                .authenticate(&tokens.access_token, "notes:read")
                .await
                .expect("unexpired original access token")
                .user_id,
            user_id
        );
        assert_eq!(
            authenticator
                .authenticate(&refreshed.access_token, "notes:read")
                .await
                .expect("refreshed access token")
                .user_id,
            user_id
        );
    }

    #[tokio::test]
    async fn projection_rebuild_keeps_the_previous_database_projection_when_a_source_is_invalid() {
        let directory = std::env::temp_dir().join(format!("marginalis-rebuild-{}", Uuid::now_v7()));
        let database = SqliteDatabase::connect("sqlite::memory:")
            .await
            .expect("database");
        let owner =
            UserId::new(EntityId::from_str("01800000-0000-7000-8000-000000000094").expect("owner"));
        database
            .oidc_identity_store()
            .register_or_lookup(
                OidcIdentity::new("https://id.example.test", "rebuild-owner", "Owner")
                    .expect("identity"),
                RegistrationPolicy::Open,
                owner,
                UnixMillis::new(0),
            )
            .await
            .expect("owner");
        let note_id =
            NoteId::new(EntityId::from_str("01800000-0000-7000-8000-000000000095").expect("note"));
        let sources = FileNoteStore::open(&directory).expect("sources");
        let source = render_note_source(
            note_id,
            owner,
            "2026-07-23T00:00:00.000Z",
            "2026-07-23T00:00:00.000Z",
            &NoteDraft {
                title: "Canonical title".into(),
                body: "searchable body".into(),
                tags: vec!["research".into()],
            },
        )
        .expect("source");
        std::fs::write(
            directory.join("notes").join(format!("{note_id}.adoc")),
            source,
        )
        .expect("write source");
        let service = ServerNoteUseCases::new(database, sources);
        assert_eq!(service.rebuild_projections().await.expect("rebuild"), 1);
        std::fs::write(
            directory.join("notes").join(format!("{note_id}.adoc")),
            "not a valid Marginalis note",
        )
        .expect("corrupt source");
        assert_eq!(
            service.rebuild_projections().await,
            Err(NoteUseCaseError::Validation)
        );
        let result = service
            .search_notes(
                Actor {
                    user_id: owner,
                    is_root: true,
                },
                "Canonical".into(),
                0,
                10,
            )
            .await
            .expect("previous projection remains searchable");
        assert_eq!(result.notes[0].note_id, note_id);
        std::fs::remove_dir_all(directory).expect("remove directory");
    }
}
