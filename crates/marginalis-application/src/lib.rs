//! Marginalis v0.3のユースケースと永続化port。
//!
//! HTTP、SQLite、OIDC clientの具体実装を参照せず、Kanidm主体とSQLite正本の境界だけを定義する。

use std::future::Future;

use async_trait::async_trait;
use marginalis_domain::{
    CanonicalActor, CanonicalAuthenticatedSession, CanonicalMcpAuthenticatedActor, CanonicalNote,
    CanonicalNoteDraft, CanonicalWebSession, EntityId, McpOAuthClient, NoteId, UnixMillis,
};

pub trait Clock: Send + Sync {
    fn now(&self) -> UnixMillis;
}

/// 実装は暗号学的に安全な乱数を使う。試験実装は決定的な値を供給できる。
pub trait Random: Send + Sync {
    fn uuid_v7(&self) -> EntityId;
    fn opaque_token(&self) -> String;
}

/// OIDC認可requestに一度だけ対応するstate、nonce、PKCE verifier。
///
/// stateはadapterでhash保存し、nonceとverifierは短い有効期間だけ保持する。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OidcLoginAttempt {
    pub state: String,
    pub nonce: String,
    pub pkce_verifier: String,
    pub expires_at: UnixMillis,
}

pub trait OidcLoginAttemptStore: Send + Sync {
    type Error: std::error::Error + Send + Sync + 'static;

    fn issue(
        &self,
        attempt: OidcLoginAttempt,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;

    fn consume(
        &self,
        state: String,
        now: UnixMillis,
    ) -> impl Future<Output = Result<Option<OidcLoginAttempt>, Self::Error>> + Send;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SessionLifetime {
    pub idle_timeout_ms: i64,
    pub absolute_timeout_ms: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuthenticationUseCaseError {
    Rejected,
    NotFound,
    Unavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NoteUseCaseError {
    NotFound,
    Forbidden,
    Conflict,
    Validation,
    Unavailable,
}

impl std::fmt::Display for NoteUseCaseError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::NotFound => "note is not available",
            Self::Forbidden => "note operation is not permitted",
            Self::Conflict => "note operation conflicts",
            Self::Validation => "note is invalid",
            Self::Unavailable => "note operation is unavailable",
        })
    }
}

impl std::error::Error for NoteUseCaseError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum McpOAuthUseCaseError {
    Rejected,
    Unavailable,
}

/// OAuth Authorization Code Flowでtransportから渡す検証済み候補。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct McpAuthorizationRequest {
    pub client_id: String,
    pub redirect_uri: String,
    pub resource_uri: String,
    pub scopes: Vec<String>,
    pub code_challenge: String,
}

/// refresh token rotationでadapterへ渡す、生成済みの新旧tokenとbinding。
pub struct McpRefreshTokenRotation {
    pub refresh_token: String,
    pub client_id: String,
    pub resource_uri: String,
    pub new_access_token: String,
    pub new_refresh_token: String,
    pub access_expires_at: UnixMillis,
    pub refresh_expires_at: UnixMillis,
}

/// SQLite正本を扱うノート操作境界。HTTP、MCP、Web UIはこの可視性規則を共有する。
#[async_trait]
pub trait NoteUseCases: Send + Sync {
    async fn list_visible_notes(
        &self,
        actor: CanonicalActor,
    ) -> Result<Vec<CanonicalNote>, NoteUseCaseError>;
    async fn read_note(
        &self,
        actor: CanonicalActor,
        note_id: NoteId,
    ) -> Result<CanonicalNote, NoteUseCaseError>;
    async fn create_note(
        &self,
        actor: CanonicalActor,
        draft: CanonicalNoteDraft,
    ) -> Result<CanonicalNote, NoteUseCaseError>;
    async fn update_note(
        &self,
        actor: CanonicalActor,
        note_id: NoteId,
        draft: CanonicalNoteDraft,
        expected_revision: i64,
    ) -> Result<CanonicalNote, NoteUseCaseError>;
    async fn soft_delete_note(
        &self,
        actor: CanonicalActor,
        note_id: NoteId,
        expected_revision: i64,
    ) -> Result<CanonicalNote, NoteUseCaseError>;
    async fn restore_note(
        &self,
        actor: CanonicalActor,
        note_id: NoteId,
        expected_revision: i64,
    ) -> Result<CanonicalNote, NoteUseCaseError>;
    fn export_note_source(&self, note: &CanonicalNote) -> Result<String, NoteUseCaseError>;
    fn render_note_html(&self, note: &CanonicalNote) -> Result<String, NoteUseCaseError>;
}

/// Kanidm groupはOIDC login時に検証し、このCookie sessionの有効期間はsnapshotとして固定する。
#[async_trait]
pub trait WebSessionUseCases: Send + Sync {
    async fn authenticate_session(
        &self,
        session_id: String,
    ) -> Result<Option<CanonicalAuthenticatedSession>, AuthenticationUseCaseError>;
    async fn verify_csrf(
        &self,
        session_id: String,
        csrf_token: String,
    ) -> Result<bool, AuthenticationUseCaseError>;
    async fn issue_session(
        &self,
        actor: CanonicalActor,
    ) -> Result<CanonicalWebSession, AuthenticationUseCaseError>;
    async fn revoke_session(&self, session_id: String) -> Result<(), AuthenticationUseCaseError>;
}

#[async_trait]
pub trait OidcAuthenticationUseCases: Send + Sync {
    async fn begin_login(&self) -> Result<String, AuthenticationUseCaseError>;
    async fn complete_login(
        &self,
        code: String,
        state: String,
    ) -> Result<CanonicalActor, AuthenticationUseCaseError>;
}

/// token endpointだけが短時間保持するtoken pair。秘密値のためDebugを実装しない。
pub struct McpTokenPair {
    pub access_token: String,
    pub refresh_token: String,
    pub access_expires_in_seconds: u64,
    pub scope: String,
}

#[async_trait]
pub trait McpOAuthUseCases: Send + Sync {
    async fn register_client(&self, client: McpOAuthClient) -> Result<(), McpOAuthUseCaseError>;
    async fn validate_authorization_request(
        &self,
        request: McpAuthorizationRequest,
    ) -> Result<McpOAuthClient, McpOAuthUseCaseError>;
    async fn authorize(
        &self,
        actor: CanonicalActor,
        request: McpAuthorizationRequest,
    ) -> Result<String, McpOAuthUseCaseError>;
    async fn exchange_authorization_code(
        &self,
        code: String,
        client_id: String,
        redirect_uri: String,
        resource_uri: String,
        verifier: String,
    ) -> Result<McpTokenPair, McpOAuthUseCaseError>;
    async fn refresh_access_token(
        &self,
        refresh_token: String,
        client_id: String,
        resource_uri: String,
    ) -> Result<McpTokenPair, McpOAuthUseCaseError>;
    async fn authenticate(
        &self,
        token: String,
        resource_uri: String,
        scope: String,
    ) -> Result<Option<CanonicalMcpAuthenticatedActor>, McpOAuthUseCaseError>;
    async fn revoke(
        &self,
        actor: CanonicalActor,
        client_id: String,
    ) -> Result<(), McpOAuthUseCaseError>;
}

