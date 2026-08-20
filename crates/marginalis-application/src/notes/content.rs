//! 文書adapterとの境界で受け渡す型とport。

use marginalis_domain::{AttachmentId, AttachmentMediaType, Note, NoteDraft, NoteId, Utf8ByteSpan};

use crate::{
    CitationStyle, NoteProfile, NoteRenderContext, NoteSourcePosition, NoteValidationDiagnostic,
    ValidatedNoteDraft,
};

/// 文書内で見つかったノート参照。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoteReferenceQuery {
    pub reference_index: usize,
    pub target_note_id: NoteId,
    pub anchor: Option<String>,
}

/// AsciiDocの画像macroが参照する、同じノート内の添付ID。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NoteAttachmentQuery {
    pub attachment_index: usize,
    pub attachment_id: AttachmentId,
    pub span: Utf8ByteSpan,
    pub position: NoteSourcePosition,
}

/// 保存済み添付を同一originの取得URLへ解決した表示入力。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoteAttachmentResolution {
    pub attachment_index: usize,
    pub href: String,
    pub media_type: AttachmentMediaType,
    pub byte_length: usize,
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

/// 本文が文献ライブラリへ問い合わせる引用1件。
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
    /// 引用の先頭位置。列はUTF-16 code unitで数えた1始まり。
    pub position: NoteSourcePosition,
}

/// 文献ライブラリで解決を終えた引用の表示。
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
    /// 一覧の中でこの項目が占める位置。
    ///
    /// 番号で示すスタイルでは、本文での初出順に振った番号が入ります。著者と年で示す
    /// スタイルでは`None`です。番号は一覧全体の性質であり、番号を持つ項目と持たない項目を
    /// 混ぜません。
    pub number: Option<u32>,
}

/// 描画時に文書adapterへ渡す解決結果一式。
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct NoteRenderInputs<'a> {
    pub references: &'a [NoteReferenceResolution],
    pub citations: &'a [NoteCitationResolution],
    pub bibliography: &'a [NoteBibliographyEntry],
    pub attachments: &'a [NoteAttachmentResolution],
}

/// 本文を省いた文書の構成。長文ノートから読む範囲を選ぶために使う。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoteOutline {
    /// 文書題名を除く見出しの一覧。出現順。
    pub sections: Vec<NoteOutlineSection>,
    /// 原文の総行数。1始まりの最終行の番号と一致する。
    pub line_count: usize,
}

/// 見出し1つと、その節が占める行範囲。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoteOutlineSection {
    /// 見出しの深さ。`==`が1。
    pub level: u8,
    pub title: String,
    /// 原文に`[#id]`と明示されたアンカー。自動生成のIDは返さない。
    pub anchor: Option<String>,
    /// 見出し行の1始まりの行番号。
    pub start_line: usize,
    /// 節の末尾の1始まりの行番号。子節を含む階層範囲で、親子の範囲は重なる。
    pub end_line: usize,
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
    fn attachment_queries(&self, body: &str) -> Result<Vec<NoteAttachmentQuery>, NoteContentError>;
    /// 本文のheaderが選んだ引用の表示規則を返す。
    ///
    /// 保存済みのノートを表示するときは下書きの検証結果が手元にないため、本文から読み直す。
    fn citation_style(&self, body: &str) -> Result<CitationStyle, NoteContentError>;
    fn has_anchor(&self, body: &str, anchor: &str) -> Result<bool, NoteContentError>;
    /// 本文を省いた文書の構成を返す。行番号は診断と同じ数え方を使う。
    fn outline(&self, body: &str) -> Result<NoteOutline, NoteContentError>;
    fn render(&self, note: &Note, inputs: NoteRenderInputs<'_>)
    -> Result<String, NoteContentError>;
    fn export(&self, note: &Note) -> Result<String, NoteContentError>;
    fn profile(&self) -> NoteProfile;
}

/// HTTPの配置方式に依存するノートと添付画像のURLを組み立てるport。
pub trait NoteLinkResolver: Send + Sync {
    /// ノート閲覧画面へのURL。
    fn href(
        &self,
        context: &NoteRenderContext,
        note_id: NoteId,
        anchor: Option<&str>,
    ) -> Option<String>;

    /// 認可付きの添付画像取得URL。
    fn attachment_href(
        &self,
        context: &NoteRenderContext,
        note_id: NoteId,
        attachment_id: AttachmentId,
    ) -> Option<String>;
}
