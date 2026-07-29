//! Marginalisのユースケースと永続化port。
//!
//! HTTP、SQLite、OIDC clientの具体実装を参照せず、Kanidm主体とSQLite正本の境界だけを定義する。

use std::future::Future;

use async_trait::async_trait;
use marginalis_domain::{
    Actor, AuthenticatedSession, EntityId, McpAuthenticatedActor, Note, NoteAccess, NoteAclEntry,
    NoteDraft, NoteId, NoteListEntry, NotePermission, NoteSummary, Revision, UnixMillis,
    WebSession,
};

mod identity;
mod notes;
mod session;
mod snapshot;

pub use identity::{
    ExternalIdentity, IdentityProvider, IdentityProviderError, OidcAuthenticationApplication,
};
pub use notes::{
    NoteAclRepository, NoteApplication, NoteCommandRepository, NoteContent, NoteContentError,
    NoteLinkResolver, NoteQueryRepository, NoteReferenceQuery, NoteReferenceResolution,
    NoteRepositoryError, NoteViewSnapshot,
};
pub use session::{SessionRepositoryError, WebSessionApplication, WebSessionRepository};
pub use snapshot::{InvalidSnapshot, LogicalSnapshot, NoteAclSnapshotEntry, RestorePlan};

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
    Unavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NoteValidationCode {
    InvalidTitle,
    InvalidTag,
    TooManyTags,
    SourceTooLarge,
    AsciiDocParseFailed,
    IncludeDirectiveDisabled,
    InlinePassthroughDisabled,
    BlockPassthroughDisabled,
    DuplicateAnchor,
    ExternalReferenceDisabled,
    InvalidNoteReference,
    InvalidUrlScheme,
    ResourceDisabled,
    UnsupportedMathLanguage,
    UnsupportedSourceLanguage,
    UnsupportedDocumentAttribute,
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
            Self::SourceTooLarge => "source_too_large",
            Self::AsciiDocParseFailed => "asciidoc_parse_failed",
            Self::IncludeDirectiveDisabled => "include_directive_disabled",
            Self::InlinePassthroughDisabled => "inline_passthrough_disabled",
            Self::BlockPassthroughDisabled => "block_passthrough_disabled",
            Self::DuplicateAnchor => "duplicate_anchor",
            Self::ExternalReferenceDisabled => "external_reference_disabled",
            Self::InvalidNoteReference => "invalid_note_reference",
            Self::InvalidUrlScheme => "invalid_url_scheme",
            Self::ResourceDisabled => "resource_disabled",
            Self::UnsupportedMathLanguage => "unsupported_math_language",
            Self::UnsupportedSourceLanguage => "unsupported_source_language",
            Self::UnsupportedDocumentAttribute => "unsupported_document_attribute",
            Self::InvalidAclSubject => "invalid_acl_subject",
            Self::DuplicateAclSubject => "duplicate_acl_subject",
            Self::OwnerInAcl => "owner_in_acl",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NoteValidationTarget {
    Source,
    Title,
    Body,
    Tag { index: usize },
    Tags,
    AclEntry { index: usize },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoteAclChange {
    pub subject: String,
    pub permission: NotePermission,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoteAclState {
    pub entries: Vec<NoteAclEntry>,
    pub revision: Revision,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Utf8ByteSpan {
    pub start: u32,
    pub end: u32,
}

/// 保存を拒否しない入力上の指摘の重大度。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NoteAdvisorySeverity {
    Warning,
    Information,
    Hint,
}

/// 入力を拒否する問題。公開時の重大度は常に`error`です。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoteValidationDiagnostic {
    pub code: String,
    pub target: NoteValidationTarget,
    pub span: Option<Utf8ByteSpan>,
    pub message: String,
}

/// 保存を拒否せず、成功したプレビューとともに返す指摘。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoteAdvisoryDiagnostic {
    pub code: String,
    pub severity: NoteAdvisorySeverity,
    pub target: NoteValidationTarget,
    pub span: Option<Utf8ByteSpan>,
    pub message: String,
}

/// 検証済みの入力と、同じ解析で得た付随情報。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidatedNoteDraft {
    pub draft: NoteDraft,
    pub diagnostics: Vec<NoteAdvisoryDiagnostic>,
    pub reference_queries: Vec<NoteReferenceQuery>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NotePreview {
    pub html: String,
    pub diagnostics: Vec<NoteAdvisoryDiagnostic>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoteProfileLimits {
    pub max_title_characters: usize,
    pub max_source_bytes: usize,
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
    pub authoring_guidance: Vec<&'static str>,
    pub allowed_source_languages: Vec<&'static str>,
    pub forbidden_rules: Vec<NoteProfileRule>,
    pub examples: Vec<NoteProfileExample>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NoteUseCaseError {
    NotFound,
    Conflict,
    Validation(Vec<NoteValidationDiagnostic>),
    RenderFailed,
    Unavailable,
}

impl std::fmt::Display for NoteUseCaseError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::NotFound => "note is not available",
            Self::Conflict => "note operation conflicts",
            Self::Validation(_) => "note is invalid",
            Self::RenderFailed => "note cannot be rendered",
            Self::Unavailable => "note operation is unavailable",
        })
    }
}

impl std::error::Error for NoteUseCaseError {}

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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoteView {
    pub note: Note,
    pub access: NoteAccess,
    pub html: String,
    pub related: RelatedNotes,
}

/// 閲覧可能なノートを取得する問い合わせ境界。
#[async_trait]
pub trait NoteQueries: Send + Sync {
    async fn list_visible_notes(
        &self,
        actor: Actor,
    ) -> Result<Vec<NoteListEntry>, NoteUseCaseError>;
    async fn read_note(&self, actor: Actor, note_id: NoteId) -> Result<Note, NoteUseCaseError>;
}

/// ノートの内容と削除状態を変更するcommand境界。
#[async_trait]
pub trait NoteCommands: Send + Sync {
    async fn create_note(&self, actor: Actor, draft: NoteDraft) -> Result<Note, NoteUseCaseError>;
    async fn update_note(
        &self,
        actor: Actor,
        note_id: NoteId,
        draft: NoteDraft,
        expected_revision: Revision,
    ) -> Result<Note, NoteUseCaseError>;
    async fn soft_delete_note(
        &self,
        actor: Actor,
        note_id: NoteId,
        expected_revision: Revision,
    ) -> Result<Note, NoteUseCaseError>;
    async fn restore_note(
        &self,
        actor: Actor,
        note_id: NoteId,
        expected_revision: Revision,
    ) -> Result<Note, NoteUseCaseError>;
}

/// ノートの検証、描画、書き出しを扱う表示境界。
#[async_trait]
pub trait NotePresentation: Send + Sync {
    async fn preview_note(
        &self,
        actor: Actor,
        draft: NoteDraft,
        context: NoteRenderContext,
    ) -> Result<NotePreview, NoteUseCaseError>;
    fn export_note_source(&self, note: &Note) -> Result<String, NoteUseCaseError>;
    async fn read_note_view(
        &self,
        actor: Actor,
        note_id: NoteId,
        context: NoteRenderContext,
    ) -> Result<NoteView, NoteUseCaseError>;
    fn note_profile(&self) -> NoteProfile;
}

/// ノートごとの直接ACLを管理する境界。
#[async_trait]
pub trait NoteAccessControl: Send + Sync {
    async fn read_note_acl(
        &self,
        actor: Actor,
        note_id: NoteId,
    ) -> Result<NoteAclState, NoteUseCaseError>;
    async fn replace_note_acl(
        &self,
        actor: Actor,
        note_id: NoteId,
        entries: Vec<NoteAclChange>,
        expected_revision: Revision,
    ) -> Result<Note, NoteUseCaseError>;
}

/// 複数transportへまとめて渡す場合のfacade。
pub trait NoteUseCases:
    NoteQueries + NoteCommands + NotePresentation + NoteAccessControl + Send + Sync
{
}

impl<T> NoteUseCases for T where
    T: NoteQueries + NoteCommands + NotePresentation + NoteAccessControl + Send + Sync
{
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum McpAccessTokenRejection {
    TokenFormat,
    StandardClaims,
    IdentityClaims,
    GroupsClaim,
    ScopeClaim,
}

impl McpAccessTokenRejection {
    pub const fn log_reason(self) -> &'static str {
        match self {
            Self::TokenFormat => "token-format",
            Self::StandardClaims => "standard-claims",
            Self::IdentityClaims => "identity-claims",
            Self::GroupsClaim => "groups-claim",
            Self::ScopeClaim => "scope-claim",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum McpAccessTokenAuthenticationError {
    Configuration,
    Discovery,
    Rejected(McpAccessTokenRejection),
    Unavailable,
}

impl core::fmt::Display for McpAccessTokenAuthenticationError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str("MCP access token authentication failed")
    }
}

impl std::error::Error for McpAccessTokenAuthenticationError {}

#[async_trait]
pub trait McpAccessTokenAuthenticator: Send + Sync {
    async fn authenticate_access_token(
        &self,
        token: String,
        resource_uri: String,
    ) -> Result<Option<McpAuthenticatedActor>, McpAccessTokenAuthenticationError>;
}
