//! transportへ公開するノート操作の型と内向き境界。

use async_trait::async_trait;
use marginalis_domain::{
    Actor, AttachmentDraft, AttachmentId, AttachmentMetadata, DeletedNoteListEntry, Identity, Note,
    NoteAccess, NoteAclEntry, NoteCreationSource, NoteDraft, NoteId, NoteListEntry, NotePermission,
    NoteReviewStatus, NoteRevisionSummary, NoteSummary, NoteValidationTarget, Revision,
    StoredAttachment, UnixMillis, Utf8ByteSpan,
};

use super::{
    NoteAttachmentQuery, NoteCitationQuery, NoteGraph, NoteGraphQuery, NoteReferenceQuery,
    NoteRevisionView,
};
use crate::{CitationStyle, MathMacro, StorageError};

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
    InvalidAttachmentReference,
    UnsupportedMathLanguage,
    UnsupportedSourceLanguage,
    UnsupportedDocumentAttribute,
    PreprocessorDirectiveDisabled,
    UnsupportedCitationStyle,
    InvalidAclIssuer,
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
            Self::InvalidAttachmentReference => "invalid_attachment_reference",
            Self::UnsupportedMathLanguage => "unsupported_math_language",
            Self::UnsupportedSourceLanguage => "unsupported_source_language",
            Self::UnsupportedDocumentAttribute => "unsupported_document_attribute",
            Self::PreprocessorDirectiveDisabled => "preprocessor_directive_disabled",
            Self::UnsupportedCitationStyle => "unsupported_citation_style",
            Self::InvalidAclIssuer => "invalid_acl_issuer",
            Self::InvalidAclSubject => "invalid_acl_subject",
            Self::DuplicateAclSubject => "duplicate_acl_subject",
            Self::OwnerInAcl => "owner_in_acl",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoteAclChange {
    pub issuer: String,
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
    /// 本文上の1始まりの行・列。列はLSP既定と同じUTF-16 code unitで数える。
    pub position: Option<NoteSourcePosition>,
    pub message: String,
}

/// 保存を拒否せず、成功したプレビューとともに返す指摘。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoteAdvisoryDiagnostic {
    pub code: String,
    pub severity: NoteAdvisorySeverity,
    pub target: NoteValidationTarget,
    pub span: Option<Utf8ByteSpan>,
    /// 本文上の1始まりの行・列。列はLSP既定と同じUTF-16 code unitで数える。
    pub position: Option<NoteSourcePosition>,
    pub message: String,
}

/// 人間向けに示す本文上の位置。
///
/// LSPの`Position`へは両方から1を引けばよい。範囲選択と他の位置符号化への変換には、
/// 診断が別に保持するUTF-8 byte spanを使用する。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NoteSourcePosition {
    pub line: u32,
    pub column: u32,
}

/// 検証済みの入力と、同じ解析で得た付随情報。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidatedNoteDraft {
    pub draft: NoteDraft,
    pub diagnostics: Vec<NoteAdvisoryDiagnostic>,
    pub reference_queries: Vec<NoteReferenceQuery>,
    pub citation_queries: Vec<NoteCitationQuery>,
    /// 本文が同じノート内の添付画像へ行う参照。
    pub attachment_queries: Vec<NoteAttachmentQuery>,
    /// 本文のheaderが選んだ引用の表示規則。属性を書かないノートは既定になる。
    pub citation_style: CitationStyle,
    /// 編集画面の装飾に使うspan注釈。原文の出現順。
    pub source_spans: Vec<NoteSourceSpan>,
}

/// 編集画面の装飾に使う、本文中の記法1件の位置。
///
/// 範囲は原文のUTF-8バイトオフセットで、診断のspanと同じ数え方を使う。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoteSourceSpan {
    pub kind: NoteSourceSpanKind,
    /// 記法全体が占める範囲。
    pub span: Utf8ByteSpan,
    /// 記法文字を除いた、装飾対象の本文部分。区別を持たない記法では`None`。
    pub content_span: Option<Utf8ByteSpan>,
    /// カーソルが離れているときに折り畳める記法文字の範囲。
    pub marker_spans: Vec<Utf8ByteSpan>,
    /// 見出しの深さ。`==`が1で、文書題名を除く。見出し以外は`None`。
    pub level: Option<u8>,
}

/// span注釈が区別する記法の種類。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NoteSourceSpanKind {
    DocumentTitle,
    Heading,
    DocumentAttribute,
    Anchor,
    Strong,
    Emphasis,
    Highlight,
    Subscript,
    Superscript,
    Monospace,
    Link,
    CrossReference,
    Citation,
    InlineMath,
    MathBlock,
    SourceBlock,
    LiteralBlock,
    Quote,
    Example,
    Admonition,
    Table,
    ListItem,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NotePreview {
    pub html: String,
    pub diagnostics: Vec<NoteAdvisoryDiagnostic>,
    pub math_macros: Vec<MathMacro>,
    /// 編集画面の装飾に使うspan注釈。原文の出現順。
    pub spans: Vec<NoteSourceSpan>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoteProfileLimits {
    pub max_title_characters: usize,
    pub max_source_bytes: usize,
    pub max_patch_bytes: usize,
    pub max_patch_hunks: usize,
    pub max_tags: usize,
    pub max_tag_characters: usize,
    pub max_attachment_bytes: usize,
    pub max_attachments_per_note: usize,
    pub max_attachment_bytes_per_note: usize,
    pub max_attachment_file_name_characters: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoteProfileRule {
    pub code: NoteValidationCode,
    pub description: &'static str,
}

/// ノートprofileで有効な、保存を妨げないAdocWeaveの規則。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoteProfileAdvisoryRule {
    pub code: &'static str,
    pub description: &'static str,
    pub severity: NoteAdvisorySeverity,
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
    pub advisory_rules: Vec<NoteProfileAdvisoryRule>,
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
    #[error("sync page limit is invalid")]
    InvalidSyncLimit,
    #[error("sync cursor is invalid")]
    InvalidSyncCursor,
    #[error("sync cursor has expired")]
    SyncCursorExpired,
    #[error("line range is outside the stored source")]
    InvalidLineRange,
    /// patchを保存済みの原文へ適用できない。理由と位置を機械可読に含む。
    #[error("patch cannot be applied: {0}")]
    PatchRejected(super::NotePatchError),
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

impl From<StorageError> for NoteUseCaseError {
    fn from(error: StorageError) -> Self {
        match error {
            StorageError::NotFound => Self::NotFound,
            StorageError::Conflict => Self::Conflict,
            StorageError::RetentionExpired => Self::RetentionExpired,
            StorageError::CorruptData => Self::CorruptData,
            StorageError::Unavailable => Self::Unavailable,
        }
    }
}

/// HTML内のノート参照と添付画像へ付与するtransport固有URLの基点。
///
/// application層はWeb UIとREST APIの経路構成を知らず、`NoteLinkResolver`が
/// この基点へ各対象の経路を加える。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoteRenderContext {
    pub base_path: String,
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

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct NoteListQuery {
    pub created_via: Option<NoteCreationSource>,
    pub review_status: Option<NoteReviewStatus>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoteReviewDetails {
    pub note_id: NoteId,
    pub current_revision: Revision,
    pub status: NoteReviewStatus,
    pub reviewed_revision: Option<Revision>,
    pub reviewed_at: Option<UnixMillis>,
    pub reviewer: Option<Identity>,
}

/// transportへ公開するノート操作の内向き境界。
///
/// RESTとMCPはどちらもこの境界全体を1つの実装から受け取るため、問い合わせ、変更、表示、
/// ACL、人手確認を別のtraitへ分けない。
#[async_trait]
pub trait NoteUseCases: Send + Sync {
    async fn list_visible_notes(
        &self,
        actor: Actor,
        query: NoteListQuery,
    ) -> Result<Vec<NoteListEntry>, NoteUseCaseError>;
    /// テンプレートノート(NOTE_TEMPLATE_TAGの付いた閲覧できるノート)の一覧。
    async fn list_note_templates(
        &self,
        actor: Actor,
    ) -> Result<Vec<NoteListEntry>, NoteUseCaseError>;
    async fn list_owned_deleted_notes(
        &self,
        actor: Actor,
    ) -> Result<Vec<DeletedNoteListEntry>, NoteUseCaseError>;
    async fn read_note(&self, actor: Actor, note_id: NoteId) -> Result<Note, NoteUseCaseError>;
    /// ノート本文を返さず、見出しの階層と行範囲を返す。
    async fn read_note_outline(
        &self,
        actor: Actor,
        note_id: NoteId,
    ) -> Result<(Note, super::NoteOutline), NoteUseCaseError>;
    /// 指定した行範囲(両端を含む1始まり)のAsciiDoc原文断片を返す。
    /// `expected_revision`を指定した場合、現在のrevisionと異なると本文を返さず競合として拒否する。
    async fn read_note_fragment(
        &self,
        actor: Actor,
        note_id: NoteId,
        start_line: usize,
        end_line: usize,
        expected_revision: Option<Revision>,
    ) -> Result<(Note, String), NoteUseCaseError>;
    /// 保存済み原文へUnified Diffを厳密に適用する。dry runでは検証まで行い保存しない。
    async fn apply_note_patch(
        &self,
        actor: Actor,
        note_id: NoteId,
        patch: &str,
        expected_revision: Revision,
        policy: NoteWritePolicy,
        dry_run: bool,
    ) -> Result<super::NotePatchApplication, NoteUseCaseError>;
    async fn create_note(
        &self,
        actor: Actor,
        draft: NoteDraft,
        policy: NoteWritePolicy,
        created_via: NoteCreationSource,
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
    async fn list_note_revisions(
        &self,
        actor: Actor,
        note_id: NoteId,
    ) -> Result<Vec<NoteRevisionSummary>, NoteUseCaseError>;
    async fn read_note_revision(
        &self,
        actor: Actor,
        note_id: NoteId,
        revision: Revision,
    ) -> Result<NoteRevisionView, NoteUseCaseError>;
    async fn compare_note_revisions(
        &self,
        actor: Actor,
        note_id: NoteId,
        from_revision: Revision,
        to_revision: Revision,
    ) -> Result<super::NoteRevisionDiff, NoteUseCaseError>;
    async fn restore_note_revision(
        &self,
        actor: Actor,
        note_id: NoteId,
        revision: Revision,
        expected_revision: Revision,
        policy: NoteWritePolicy,
    ) -> Result<Note, NoteUseCaseError>;
    async fn upload_note_attachment(
        &self,
        actor: Actor,
        note_id: NoteId,
        draft: AttachmentDraft,
    ) -> Result<AttachmentMetadata, NoteUseCaseError>;
    async fn list_note_attachments(
        &self,
        actor: Actor,
        note_id: NoteId,
    ) -> Result<Vec<AttachmentMetadata>, NoteUseCaseError>;
    async fn read_note_attachment(
        &self,
        actor: Actor,
        note_id: NoteId,
        attachment_id: AttachmentId,
    ) -> Result<StoredAttachment, NoteUseCaseError>;
    async fn delete_unused_note_attachment(
        &self,
        actor: Actor,
        note_id: NoteId,
        attachment_id: AttachmentId,
    ) -> Result<(), NoteUseCaseError>;
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
    async fn read_note_review(
        &self,
        actor: Actor,
        note_id: NoteId,
    ) -> Result<NoteReviewDetails, NoteUseCaseError>;
    async fn mark_note_reviewed(
        &self,
        actor: Actor,
        note_id: NoteId,
        expected_revision: Revision,
    ) -> Result<NoteReviewDetails, NoteUseCaseError>;
    async fn sync_notes(
        &self,
        actor: Actor,
        cursor: Option<String>,
        limit: Option<usize>,
    ) -> Result<super::NoteSyncPage, NoteUseCaseError>;
}
