//! サーバーの設定境界。環境変数とNixOS moduleはこの型へ変換される。

use core::fmt;
use std::{env, net::SocketAddr, path::PathBuf, time::Duration};

use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use marginalis_application::{
    AuthenticationUseCaseError, Clock, McpAccessTokenStore, McpAuthorizationRequest, McpOAuthStore,
    McpOAuthUseCaseError, McpOAuthUseCases, McpRefreshTokenRotation, McpTokenPair, NoteAclService,
    NoteAclServiceError, NoteAclStore, NoteOperationKind, NoteQueryStore, NoteUseCaseError,
    NoteUseCases, NoteWriteService, OidcUserAdministrationStore, Random, RootCredentialStore,
    SessionLifetime, WebAuthenticationUseCases, WebSession, WebSessionService, WebSessionStore,
};
use marginalis_auth_oidc::{OidcAuthentication, OidcCallbackError};
use marginalis_domain::{
    Actor, EntityId, McpAuthorizationGrant, NoteId, NotePage, NotePermission, NoteSource,
    OidcLoginResult, SourceRevision, UnixMillis, UserId,
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
}

/// Web session、外部OIDCとroot管理を同じapplication境界で公開するserver adapter。
#[derive(Clone)]
pub struct ServerWebAuthenticationUseCases {
    database: SqliteDatabase,
    oidc: Option<OidcAuthentication>,
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
    async fn authenticate_read(&self, bearer_token: &str) -> Result<Actor, McpAuthenticationError> {
        self.database
            .mcp_access_token_store()
            .authenticate_read(
                bearer_token.into(),
                self.resource_uri.clone(),
                SystemClock.now(),
            )
            .await
            .map_err(|_| McpAuthenticationError::Unavailable)?
            .ok_or(McpAuthenticationError::MissingOrInvalid)
    }
}

impl ServerWebAuthenticationUseCases {
    pub const fn new(database: SqliteDatabase) -> Self {
        Self {
            database,
            oidc: None,
        }
    }

    pub fn with_oidc(database: SqliteDatabase, oidc: OidcAuthentication) -> Self {
        Self {
            database,
            oidc: Some(oidc),
        }
    }

    fn oidc(&self) -> Result<&OidcAuthentication, AuthenticationUseCaseError> {
        self.oidc
            .as_ref()
            .ok_or(AuthenticationUseCaseError::Unavailable)
    }
}

#[async_trait]
impl WebAuthenticationUseCases for ServerWebAuthenticationUseCases {
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
        self.oidc()?
            .complete_login(
                &self.database.oidc_login_attempt_store(),
                &self.database.oidc_identity_store(),
                &SystemRandom,
                &SystemClock,
                &code,
                &state,
            )
            .await
            .map_err(|error| match error {
                OidcCallbackError::Rejected(_) => AuthenticationUseCaseError::Rejected,
                OidcCallbackError::Unavailable => AuthenticationUseCaseError::Unavailable,
            })
    }

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
            SessionLifetime {
                idle_timeout_ms: 8 * 60 * 60 * 1_000,
                absolute_timeout_ms: 7 * 24 * 60 * 60 * 1_000,
            },
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
            return Ok(None);
        };
        WebSessionService::new(
            &self.database.web_session_store(),
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
        .map(Some)
        .map_err(|_| AuthenticationUseCaseError::Unavailable)
    }

    async fn revoke_session(&self, session_id: String) -> Result<(), AuthenticationUseCaseError> {
        self.database
            .web_session_store()
            .revoke(session_id, SystemClock.now())
            .await
            .map_err(|_| AuthenticationUseCaseError::Unavailable)
    }

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
        user_id: UserId,
    ) -> Result<bool, AuthenticationUseCaseError> {
        self.database
            .oidc_user_administration_store()
            .activate(user_id, SystemClock.now())
            .await
            .map_err(|_| AuthenticationUseCaseError::Unavailable)
    }

    fn cookie_path(&self) -> &str {
        self.oidc
            .as_ref()
            .map_or("/", OidcAuthentication::cookie_path)
    }
}

impl ServerNoteUseCases {
    pub const fn new(database: SqliteDatabase, sources: FileNoteStore) -> Self {
        Self { database, sources }
    }

    pub async fn recover(&self) -> Result<(), NoteUseCaseError> {
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
        let projection = marginalis_asciidoc::parse_note_projection(source)
            .map_err(|_| NoteUseCaseError::Unavailable)?;
        Ok(NoteSource {
            note_id,
            title: projection.title,
            revision: SourceRevision::from_source(&content),
            content,
        })
    }

    async fn create_source(
        &self,
        actor: Actor,
        source: String,
    ) -> Result<NoteId, NoteUseCaseError> {
        let projection = marginalis_asciidoc::parse_note_projection(&source)
            .map_err(|_| NoteUseCaseError::Validation)?;
        if projection.owner_id != actor.user_id {
            return Err(NoteUseCaseError::Forbidden);
        }
        if self
            .sources
            .read(projection.note_id)
            .map_err(|_| NoteUseCaseError::Unavailable)?
            .is_some()
        {
            return Err(NoteUseCaseError::Conflict);
        }
        let note_id = projection.note_id;
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

    async fn update_source(
        &self,
        actor: Actor,
        note_id: NoteId,
        source: String,
        expected_revision: SourceRevision,
    ) -> Result<(), NoteUseCaseError> {
        let permission = self
            .database
            .note_acl_store()
            .permission_for(actor, note_id)
            .await
            .map_err(|_| NoteUseCaseError::Unavailable)?;
        if !matches!(permission, Some(value) if value.permits(NotePermission::Write)) {
            return Err(NoteUseCaseError::NotFound);
        }
        let projection = marginalis_asciidoc::parse_note_projection(&source)
            .map_err(|_| NoteUseCaseError::Validation)?;
        if projection.note_id != note_id {
            return Err(NoteUseCaseError::Validation);
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
        let previous_projection = marginalis_asciidoc::parse_note_projection(previous_source)
            .map_err(|_| NoteUseCaseError::Unavailable)?;
        if projection.owner_id != previous_projection.owner_id {
            return Err(NoteUseCaseError::Validation);
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
        .replace(NoteOperationKind::Update, projection, source.into_bytes())
        .await
        .map(|_| ())
        .map_err(|_| NoteUseCaseError::Unavailable)
    }

    async fn delete_note(
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServerConfig {
    pub base_url: Url,
    pub listen_address: SocketAddr,
    pub data_dir: PathBuf,
    pub database_url: String,
    pub oidc: OidcPublicConfig,
    pub mcp_enabled: bool,
    pub mcp_client_metadata_allowed_hosts: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OidcPublicConfig {
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
        let data_dir = PathBuf::from(required("MARGINALIS_DATA_DIR")?);
        if data_dir.as_os_str().is_empty() {
            return Err(ConfigurationError::EmptyDataDirectory);
        }
        let listen_address = required("MARGINALIS_LISTEN_ADDR")?
            .parse()
            .map_err(|_| ConfigurationError::InvalidListenAddress)?;
        let configuration = Self {
            base_url,
            listen_address,
            data_dir,
            database_url: required("MARGINALIS_DATABASE_URL")?,
            oidc: OidcPublicConfig {
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
}
