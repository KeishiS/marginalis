//! Marginalisのユースケースと永続化port。
//!
//! HTTP、SQLite、OIDC clientの具体実装を参照せず、Kanidm主体とSQLite正本の境界だけを定義する。

use std::future::Future;

use async_trait::async_trait;
use marginalis_domain::{
    Actor, AuthenticatedSession, DeletedNoteListEntry, EntityId, Note, NoteAccess, NoteAclEntry,
    NoteDraft, NoteId, NoteListEntry, NotePermission, NoteSummary, NoteValidationTarget, Revision,
    UnixMillis, Utf8ByteSpan, WebSession,
};
pub use mcp_authorization_server::{
    AuthenticatedPrincipal as McpAuthenticatedPrincipal,
    AuthorizationClient as McpAuthorizationClient,
    AuthorizationCodeExchange as McpAuthorizationCodeExchange,
    AuthorizationError as McpOAuthUseCaseError, AuthorizationGrant as McpAuthorizationGrant,
    AuthorizationRequest as McpAuthorizationRequest, Client as McpOAuthClient,
    ClientMetadataResolver as McpClientMetadataResolver,
    ClientMetadataResolverError as McpClientMetadataResolverError,
    ClientRegistrationMethod as McpClientRegistrationMethod, Principal as McpPrincipal,
    RefreshTokenRotation as McpRefreshTokenRotation,
    RefreshTokenRotationOutcome as McpRefreshTokenRotationOutcome,
    RegisteredClient as McpRegisteredOAuthClient, Repository as McpOAuthRepository,
    RepositoryError as McpOAuthRepositoryError, ResolvedRedirectUri as McpResolvedRedirectUri,
    ResourcePolicy as McpResourcePolicy, Timestamp as McpTimestamp, TokenPair as McpTokenPair,
    ValidatedAuthorizationRequest as McpValidatedAuthorizationRequest,
};

mod bibliography;
mod citation;
mod identity;
mod math_macros;
mod mcp_oauth;
mod notes;
mod session;
mod snapshot;

pub use bibliography::{
    BibliographyApplication, BibliographyRepository, BibliographyRepositoryError,
    BibliographyUseCaseError, BibliographyUseCases,
};
pub use citation::CitationStyle;
pub use identity::{
    ExternalIdentity, IdentityProvider, IdentityProviderError, OidcAuthenticationApplication,
};
pub use math_macros::{
    MathMacro, MathMacroApplication, MathMacroRepository, MathMacroRepositoryError,
    MathMacroSettings, MathMacroUseCaseError, MathMacroUseCases, validate_math_macros,
};
pub use mcp_oauth::McpOAuthApplication;
pub use notes::{
    AccessibleNote, NoteAclRepository, NoteApplication, NoteBibliographyEntry, NoteCitationQuery,
    NoteCitationResolution, NoteCitationSegment, NoteCommandRepository, NoteContent,
    NoteContentError, NoteGraph, NoteGraphCitation, NoteGraphNote, NoteGraphQuery,
    NoteGraphReference, NoteGraphWork, NoteLinkResolver, NoteLinks, NoteQueryRepository,
    NoteReferenceQuery, NoteReferenceResolution, NoteRenderInputs, NoteRepositoryError,
    NoteViewSnapshot,
};
pub use session::{SessionRepositoryError, WebSessionApplication, WebSessionRepository};
pub use snapshot::{
    InvalidSnapshot, LogicalSnapshot, MathMacroSettingsSnapshot, NoteAclSnapshotEntry, RestorePlan,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum AuthenticationUseCaseError {
    #[error("authentication was rejected")]
    Rejected,
    #[error("authentication is unavailable")]
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
    PreprocessorDirectiveDisabled,
    UnsupportedCitationStyle,
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
            Self::PreprocessorDirectiveDisabled => "preprocessor_directive_disabled",
            Self::UnsupportedCitationStyle => "unsupported_citation_style",
            Self::InvalidAclSubject => "invalid_acl_subject",
            Self::DuplicateAclSubject => "duplicate_acl_subject",
            Self::OwnerInAcl => "owner_in_acl",
        }
    }
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

/// 保存を拒否しない入力上の指摘の重大度。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NoteAdvisorySeverity {
    Warning,
    Information,
    Hint,
}

/// ノートの変更時に、保存を妨げない診断をどこまで許容するか。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NoteWritePolicy {
    AllowAdvisories,
    RejectWarnings,
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
    pub citation_queries: Vec<NoteCitationQuery>,
    /// 本文のheaderが選んだ引用の表示規則。属性を書かないノートは既定になる。
    pub citation_style: CitationStyle,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NotePreview {
    pub html: String,
    pub diagnostics: Vec<NoteAdvisoryDiagnostic>,
    pub math_macros: Vec<MathMacro>,
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
    /// 文書headerへ書ける文書属性の名前。入力検査と同じ一覧から導きます。
    pub allowed_document_attributes: Vec<&'static str>,
    /// 引用の表示スタイルとして選べる値。先頭が既定です。
    pub allowed_citation_styles: Vec<&'static str>,
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

/// ノート操作の失敗理由。
///
/// ここでの文言は開発者向けの記録用であり、利用者向けの`code`と`message`は
/// transport側の写像が決める。両者を混同しないこと。
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum NoteUseCaseError {
    #[error("note is not available")]
    NotFound,
    #[error("note operation conflicts")]
    Conflict,
    #[error("note restoration period has expired")]
    RetentionExpired,
    #[error("note is invalid")]
    Validation(Vec<NoteValidationDiagnostic>),
    #[error("note input contains warnings")]
    AdvisoriesRejected(Vec<NoteAdvisoryDiagnostic>),
    #[error("note cannot be rendered")]
    RenderFailed,
    /// 一時的に処理できない。再試行で解消しうる。
    #[error("note operation is unavailable")]
    Unavailable,
    /// 保存済みの内容が現行の規則を満たさない。再試行では解消しない。
    ///
    /// 利用者向けの応答は`Unavailable`と同じにして内部状態を開示しないが、運用時に
    /// 一時障害と区別できるよう型では分ける。
    #[error("stored note data is invalid")]
    CorruptData,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct McpAuthenticatedActor {
    pub actor: Actor,
    pub scopes: Vec<String>,
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoteView {
    pub note: Note,
    pub access: NoteAccess,
    pub html: String,
    pub related: RelatedNotes,
    pub math_macros: Vec<MathMacro>,
}

/// 閲覧可能なノートを取得する問い合わせ境界。
#[async_trait]
pub trait NoteQueries: Send + Sync {
    async fn list_visible_notes(
        &self,
        actor: Actor,
    ) -> Result<Vec<NoteListEntry>, NoteUseCaseError>;
    async fn list_owned_deleted_notes(
        &self,
        actor: Actor,
    ) -> Result<Vec<DeletedNoteListEntry>, NoteUseCaseError>;
    async fn read_note(&self, actor: Actor, note_id: NoteId) -> Result<Note, NoteUseCaseError>;
}

/// ノートの内容と削除状態を変更するcommand境界。
#[async_trait]
pub trait NoteCommands: Send + Sync {
    async fn create_note(
        &self,
        actor: Actor,
        draft: NoteDraft,
        policy: NoteWritePolicy,
    ) -> Result<Note, NoteUseCaseError>;
    async fn update_note(
        &self,
        actor: Actor,
        note_id: NoteId,
        draft: NoteDraft,
        expected_revision: Revision,
        policy: NoteWritePolicy,
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
    async fn preview_new_note(
        &self,
        actor: Actor,
        draft: NoteDraft,
        context: NoteRenderContext,
    ) -> Result<NotePreview, NoteUseCaseError>;
    async fn preview_note_update(
        &self,
        actor: Actor,
        note_id: NoteId,
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
    /// 閲覧できるノートと、それらが引用する文献の関係を返す。
    async fn read_note_graph(
        &self,
        actor: Actor,
        query: NoteGraphQuery,
    ) -> Result<NoteGraph, NoteUseCaseError>;
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
    /// 同じ要求から解決した`resolved`を使い、clientを再取得せず残りの項目を検証する。
    async fn validate_resolved_authorization_request(
        &self,
        request: McpAuthorizationRequest,
        resolved: McpAuthorizationClient,
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
    async fn revoke_token(
        &self,
        token: String,
        client_id: String,
    ) -> Result<(), McpOAuthUseCaseError>;
}
