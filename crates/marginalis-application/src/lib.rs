//! Marginalisのユースケースと永続化port。
//!
//! HTTP、SQLite、OIDC clientの具体実装を参照せず、Kanidm主体とSQLite正本の境界だけを定義する。

use std::future::Future;

use async_trait::async_trait;
use marginalis_domain::{
    Actor, AuthenticatedSession, EntityId, McpAuthenticatedActor, McpAuthorizationGrant,
    McpOAuthClient, Note, NoteAclEntry, NoteCapabilities, NoteDraft, NoteId, NoteSummary,
    UnixMillis, WebSession,
};

mod identity;
mod mcp_oauth;
mod notes;
mod session;

pub use identity::{
    ExternalIdentity, IdentityProvider, IdentityProviderError, OidcAuthenticationApplication,
};
pub use mcp_oauth::{McpOAuthApplication, McpOAuthRepository, McpOAuthRepositoryError};
pub use notes::{
    NoteApplication, NoteContent, NoteContentError, NoteLinkResolver, NoteReferenceQuery,
    NoteReferenceResolution, NoteRepository, NoteRepositoryError,
};
pub use session::{SessionRepositoryError, WebSessionApplication, WebSessionRepository};

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
        now: UnixMillis,
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
pub enum NoteValidationCode {
    InvalidTitle,
    InvalidTag,
    TooManyTags,
    BodyTooLarge,
    AsciiDocParseFailed,
    IncludeDirectiveDisabled,
    InlinePassthroughDisabled,
    BlockPassthroughDisabled,
    DuplicateAnchor,
    ExternalReferenceDisabled,
    InvalidUrlScheme,
    ResourceDisabled,
    UnsupportedMathLanguage,
    UnsupportedSourceLanguage,
    InvalidAclSubject,
    DuplicateAclSubject,
    OwnerInAcl,
}

impl NoteValidationCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidTitle => "invalid_title",
            Self::InvalidTag => "invalid_tag",
            Self::TooManyTags => "too_many_tags",
            Self::BodyTooLarge => "body_too_large",
            Self::AsciiDocParseFailed => "asciidoc_parse_failed",
            Self::IncludeDirectiveDisabled => "include_directive_disabled",
            Self::InlinePassthroughDisabled => "inline_passthrough_disabled",
            Self::BlockPassthroughDisabled => "block_passthrough_disabled",
            Self::DuplicateAnchor => "duplicate_anchor",
            Self::ExternalReferenceDisabled => "external_reference_disabled",
            Self::InvalidUrlScheme => "invalid_url_scheme",
            Self::ResourceDisabled => "resource_disabled",
            Self::UnsupportedMathLanguage => "unsupported_math_language",
            Self::UnsupportedSourceLanguage => "unsupported_source_language",
            Self::InvalidAclSubject => "invalid_acl_subject",
            Self::DuplicateAclSubject => "duplicate_acl_subject",
            Self::OwnerInAcl => "owner_in_acl",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NoteValidationTarget {
    Title,
    Body,
    Tag { index: usize },
    Tags,
    AclEntry { index: usize },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Utf8ByteSpan {
    pub start: u32,
    pub end: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoteValidationDiagnostic {
    pub code: NoteValidationCode,
    pub target: NoteValidationTarget,
    pub span: Option<Utf8ByteSpan>,
    pub message: &'static str,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoteProfileLimits {
    pub max_title_characters: usize,
    pub max_body_bytes: usize,
    pub max_tags: usize,
    pub max_tag_characters: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoteProfileRule {
    pub code: NoteValidationCode,
    pub description: &'static str,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoteProfileExample {
    pub kind: &'static str,
    pub description: &'static str,
    pub body: &'static str,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoteProfileSyntax {
    /// 主要な対応blockの案内。許可集合を網羅する一覧ではありません。
    pub common_blocks: Vec<&'static str>,
    /// 主要な対応inlineの案内。許可集合を網羅する一覧ではありません。
    pub common_inlines: Vec<&'static str>,
    pub source_language_optional: bool,
    pub allowed_math_languages: Vec<&'static str>,
    pub title_forbidden: Vec<&'static str>,
    pub tag_forbidden: Vec<&'static str>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoteProfileNormalization {
    pub title: Vec<&'static str>,
    pub tags: Vec<&'static str>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoteProfile {
    pub profile_version: u32,
    pub adocweave_package_version: &'static str,
    pub limits: NoteProfileLimits,
    pub normalization: NoteProfileNormalization,
    pub syntax: NoteProfileSyntax,
    pub allowed_source_languages: Vec<&'static str>,
    pub forbidden_rules: Vec<NoteProfileRule>,
    pub examples: Vec<NoteProfileExample>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NoteUseCaseError {
    NotFound,
    Forbidden,
    Conflict,
    Validation(Vec<NoteValidationDiagnostic>),
    Unavailable,
}

impl std::fmt::Display for NoteUseCaseError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::NotFound => "note is not available",
            Self::Forbidden => "note operation is not permitted",
            Self::Conflict => "note operation conflicts",
            Self::Validation(_) => "note is invalid",
            Self::Unavailable => "note operation is unavailable",
        })
    }
}

impl std::error::Error for NoteUseCaseError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum McpOAuthUseCaseError {
    InvalidRequest,
    InvalidClient,
    InvalidRedirectUri,
    InvalidScope,
    InvalidTarget,
    InvalidGrant,
    Unavailable,
}

/// OAuth Authorization Code Flowでtransportから渡す検証済み候補。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct McpAuthorizationRequest {
    pub client_id: String,
    pub redirect_uri: Option<String>,
    pub resource_uri: String,
    pub scopes: Vec<String>,
    pub code_challenge: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct McpAuthorizationClient {
    pub client: McpOAuthClient,
    pub redirect_uri: String,
}

/// client登録とredirect URIを照合し、既定scopeも解決した認可要求。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct McpValidatedAuthorizationRequest {
    pub client: McpOAuthClient,
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
    pub requested_scopes: Option<Vec<String>>,
    pub new_access_token: String,
    pub new_refresh_token: String,
    pub access_expires_at: UnixMillis,
    pub refresh_expires_at: UnixMillis,
}

/// 認可codeの一回消費とtoken pair発行を同じtransactionで行うための入力。
pub struct McpAuthorizationCodeExchange {
    pub code: String,
    pub client_id: String,
    pub redirect_uri: Option<String>,
    pub resource_uri: String,
    pub code_challenge: String,
    pub access_token: String,
    pub refresh_token: String,
    pub access_expires_at: UnixMillis,
    pub refresh_expires_at: UnixMillis,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum McpRefreshTokenRotationOutcome {
    Rotated {
        grant: McpAuthorizationGrant,
        access_scopes: Vec<String>,
    },
    InvalidToken,
    InvalidScope,
}

/// HTML内のノート参照へ付与するtransport固有の公開パス。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoteRenderContext {
    pub note_path_prefix: String,
}

/// 閲覧中のノートと明示的な参照で直接つながる、現在の利用者に可視なノート。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RelatedNotes {
    pub outgoing: Vec<NoteSummary>,
    pub incoming: Vec<NoteSummary>,
}

/// SQLite正本を扱うノート操作境界。HTTP、MCP、Web UIはこの可視性規則を共有する。
#[async_trait]
pub trait NoteUseCases: Send + Sync {
    async fn list_visible_notes(&self, actor: Actor) -> Result<Vec<Note>, NoteUseCaseError>;
    async fn read_note(&self, actor: Actor, note_id: NoteId) -> Result<Note, NoteUseCaseError>;
    async fn create_note(&self, actor: Actor, draft: NoteDraft) -> Result<Note, NoteUseCaseError>;
    async fn update_note(
        &self,
        actor: Actor,
        note_id: NoteId,
        draft: NoteDraft,
        expected_revision: i64,
    ) -> Result<Note, NoteUseCaseError>;
    async fn preview_note(
        &self,
        actor: Actor,
        draft: NoteDraft,
        context: NoteRenderContext,
    ) -> Result<String, NoteUseCaseError>;
    async fn soft_delete_note(
        &self,
        actor: Actor,
        note_id: NoteId,
        expected_revision: i64,
    ) -> Result<Note, NoteUseCaseError>;
    async fn restore_note(
        &self,
        actor: Actor,
        note_id: NoteId,
        expected_revision: i64,
    ) -> Result<Note, NoteUseCaseError>;
    fn export_note_source(&self, note: &Note) -> Result<String, NoteUseCaseError>;
    async fn render_note_html(
        &self,
        actor: Actor,
        note_id: NoteId,
        context: NoteRenderContext,
    ) -> Result<String, NoteUseCaseError>;
    async fn related_notes(
        &self,
        actor: Actor,
        note_id: NoteId,
    ) -> Result<RelatedNotes, NoteUseCaseError>;
    async fn note_capabilities(
        &self,
        actor: Actor,
        note_id: NoteId,
    ) -> Result<NoteCapabilities, NoteUseCaseError>;
    async fn read_note_acl(
        &self,
        actor: Actor,
        note_id: NoteId,
    ) -> Result<Vec<NoteAclEntry>, NoteUseCaseError>;
    async fn replace_note_acl(
        &self,
        actor: Actor,
        note_id: NoteId,
        entries: Vec<NoteAclEntry>,
        expected_revision: i64,
    ) -> Result<Note, NoteUseCaseError>;
    fn note_profile(&self) -> NoteProfile;
}

/// Kanidm groupはOIDC login時に検証し、このCookie sessionの有効期間はsnapshotとして固定する。
#[async_trait]
pub trait WebSessionUseCases: Send + Sync {
    async fn authenticate_session(
        &self,
        session_id: String,
    ) -> Result<Option<AuthenticatedSession>, AuthenticationUseCaseError>;
    async fn verify_csrf(
        &self,
        session_id: String,
        csrf_token: String,
    ) -> Result<bool, AuthenticationUseCaseError>;
    async fn issue_session(&self, actor: Actor) -> Result<WebSession, AuthenticationUseCaseError>;
    async fn revoke_session(&self, session_id: String) -> Result<(), AuthenticationUseCaseError>;
}

#[async_trait]
pub trait OidcAuthenticationUseCases: Send + Sync {
    async fn begin_login(&self) -> Result<String, AuthenticationUseCaseError>;
    async fn complete_login(
        &self,
        code: String,
        state: String,
    ) -> Result<Actor, AuthenticationUseCaseError>;
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
    async fn resolve_authorization_client(
        &self,
        client_id: String,
        redirect_uri: Option<String>,
    ) -> Result<McpAuthorizationClient, McpOAuthUseCaseError>;
    async fn validate_authorization_request(
        &self,
        request: McpAuthorizationRequest,
    ) -> Result<McpValidatedAuthorizationRequest, McpOAuthUseCaseError>;
    async fn authorize(
        &self,
        actor: Actor,
        request: McpValidatedAuthorizationRequest,
    ) -> Result<String, McpOAuthUseCaseError>;
    async fn exchange_authorization_code(
        &self,
        code: String,
        client_id: String,
        redirect_uri: Option<String>,
        resource_uri: String,
        verifier: String,
    ) -> Result<McpTokenPair, McpOAuthUseCaseError>;
    async fn refresh_access_token(
        &self,
        refresh_token: String,
        client_id: String,
        resource_uri: String,
        scopes: Option<Vec<String>>,
    ) -> Result<McpTokenPair, McpOAuthUseCaseError>;
    async fn authenticate(
        &self,
        token: String,
        resource_uri: String,
    ) -> Result<Option<McpAuthenticatedActor>, McpOAuthUseCaseError>;
    async fn revoke(&self, actor: Actor, client_id: String) -> Result<(), McpOAuthUseCaseError>;
}
