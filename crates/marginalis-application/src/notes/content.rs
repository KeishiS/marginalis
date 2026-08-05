//! 文書adapterとの境界で受け渡す型とport。

use marginalis_domain::{Note, NoteDraft, NoteId, Utf8ByteSpan};

use crate::{
    CitationStyle, NoteProfile, NoteRenderContext, NoteValidationDiagnostic, ValidatedNoteDraft,
};

/// 文書内で見つかったノート参照。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoteReferenceQuery {
    pub reference_index: usize,
    pub target_note_id: NoteId,
    pub anchor: Option<String>,
}

/// 認可と外部URLの解決を終えたノート参照。
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NoteReferenceResolution {
    Visible {
        reference_index: usize,
        href: String,
        title: String,
        missing_anchor: bool,
    },
    Hidden {
        reference_index: usize,
    },
}

/// 本文が書誌ライブラリーへ問い合わせる引用1件。
///
/// `cite:[a, b]`のように1つの引用が複数のcitation keyを名指すため、keyは並びで持つ。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoteCitationQuery {
    pub citation_index: usize,
    pub keys: Vec<String>,
    /// `locator="p. 12"`のように引用へ添えられた位置。
    pub locator: Option<String>,
    /// 本文中で引用が占める範囲。診断の位置に使う。
    pub span: Utf8ByteSpan,
}

/// 書誌ライブラリーで解決を終えた引用の表示。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoteCitationResolution {
    pub citation_index: usize,
    pub segments: Vec<NoteCitationSegment>,
}

/// 引用表示のうち、link先を共有する連続した一区切り。
///
/// `(Smith 2024; Tanaka 2025)`のように、括弧と区切りは素の文字列のまま、著者名だけを
/// 参考文献項目へlinkさせるために分ける。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoteCitationSegment {
    pub text: String,
    /// link先の参考文献項目のanchor。`None`は素の文字列として表示する。
    pub anchor: Option<String>,
}

/// 描画時に生成する参考文献一覧の1項目。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoteBibliographyEntry {
    pub citation_key: String,
    pub text: String,
    /// 項目の見出しとして表示する短い文字列。
    ///
    /// 番号で示すスタイルでは初出順の番号が入ります。文書adapterはこの値を記法として
    /// 解釈せず、参考文献項目の補助的な表示名として描画器へ渡します。
    pub label: Option<String>,
}

/// 描画時に文書adapterへ渡す解決結果一式。
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct NoteRenderInputs<'a> {
    pub references: &'a [NoteReferenceResolution],
    pub citations: &'a [NoteCitationResolution],
    pub bibliography: &'a [NoteBibliographyEntry],
}

/// 文書adapterが保存済みの内容を解析または変換できない場合の失敗。
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("note content could not be processed")]
pub struct NoteContentError;

/// AsciiDocなどの文書形式に依存する処理を受け持つport。
pub trait NoteContent: Send + Sync {
    fn validate_draft(
        &self,
        draft: NoteDraft,
    ) -> Result<ValidatedNoteDraft, Vec<NoteValidationDiagnostic>>;
    fn reference_queries(&self, body: &str) -> Result<Vec<NoteReferenceQuery>, NoteContentError>;
    fn citation_queries(&self, body: &str) -> Result<Vec<NoteCitationQuery>, NoteContentError>;
    /// 本文のheaderが選んだ引用の表示規則を返す。
    ///
    /// 保存済みのノートを表示するときは下書きの検証結果が手元にないため、本文から読み直す。
    fn citation_style(&self, body: &str) -> Result<CitationStyle, NoteContentError>;
    fn has_anchor(&self, body: &str, anchor: &str) -> Result<bool, NoteContentError>;
    fn render(&self, note: &Note, inputs: NoteRenderInputs<'_>)
    -> Result<String, NoteContentError>;
    fn export(&self, note: &Note) -> Result<String, NoteContentError>;
    fn profile(&self) -> NoteProfile;
}

/// HTTPの配置方式に依存するノートURLを組み立てるport。
pub trait NoteLinkResolver: Send + Sync {
    fn href(
        &self,
        context: &NoteRenderContext,
        note_id: NoteId,
        anchor: Option<&str>,
    ) -> Option<String>;
}
