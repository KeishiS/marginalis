//! ノート入力に適用する規則の単一正本。
//!
//! ここで定義した値から、AsciiDoc解析器へ渡す設定、公開JSON Schema、診断の説明文、
//! MCPの`get_note_profile`が返す内容をすべて導出する。同じ規則を別の場所へ書き写さない。

/// ノート入力の上限と許可範囲。
///
/// 値は[`NOTE_POLICY`]が唯一の実体である。この型は、規則を受け取る側が何に依存しているかを
/// 型で示すためにある。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NotePolicy {
    /// 文書題名の最大文字数。
    pub max_title_characters: usize,
    /// AsciiDoc文書のUTF-8バイト数の上限。
    pub max_source_bytes: usize,
    /// MCPの`apply_note_patch`が受け取るUnified DiffのUTF-8バイト数の上限。
    pub max_patch_bytes: usize,
    /// 一つのUnified Diffに含められるhunk数の上限。
    pub max_patch_hunks: usize,
    /// 一つのノートへ付けられるタグの最大数。
    pub max_tags: usize,
    /// タグ一つあたりの最大文字数。
    pub max_tag_characters: usize,
    /// 描画したHTMLのバイト数の上限。
    pub max_output_bytes: u32,
    /// コードブロックで指定できる言語。
    pub allowed_source_languages: &'static [&'static str],
    /// 数式で指定できる言語。
    pub allowed_math_languages: &'static [&'static str],
    /// HTMLのclassとして描画へ残す、数学文書用のblock role。
    ///
    /// 入力したroleは利用者が指定するclass名になるため、画面側が表示規則を持つ名前だけを
    /// HTMLへ残します。HTML描画と`get_note_profile`の案内はこの一覧から導きます。
    pub allowed_mathematical_block_roles: &'static [&'static str],
    /// 本文に記述できるリンクのURLスキーム。
    pub allowed_url_schemes: &'static [&'static str],
    /// 文書headerへ書ける文書属性の名前。
    ///
    /// 入力検査と`get_note_profile`の広告は、どちらもこの一覧から導きます。片方だけを直すと、
    /// 受理する入力と公開する制約が食い違います。
    ///
    /// Marginalisが独自に決めた属性は[`DOCUMENT_ATTRIBUTE_PREFIX`]で始めます。AsciiDocの
    /// 組込み属性と名前で区別できるようにするためです。
    pub allowed_document_attributes: &'static [&'static str],
    /// [`CITATION_STYLE_DOCUMENT_ATTRIBUTE`]へ書ける値。
    ///
    /// 先頭が、属性を書かないノートに使う既定のスタイルです。任意のCSLスタイル名は受け付けず、
    /// 検証済みの組込みスタイルだけを選べるようにします。
    pub allowed_citation_styles: &'static [&'static str],
}

/// Marginalis独自の文書属性に付ける接頭辞。
///
/// AsciiDocの言語仕様は独自属性の命名規則を定めていないため、Marginalisが決めます。接頭辞が
/// あると、その属性が他のAsciiDoc処理系では意味を持たないことが本文を読むだけで分かります。
/// AsciiDocが後から同じ名前の組込み属性を定義しても、意味が衝突しません。
pub const DOCUMENT_ATTRIBUTE_PREFIX: &str = "marginalis-";

/// ノートのタグを並べる文書属性の名前。
pub const TAGS_DOCUMENT_ATTRIBUTE: &str = "marginalis-tags";

/// 引用の表示スタイルを選ぶ文書属性の名前。
pub const CITATION_STYLE_DOCUMENT_ATTRIBUTE: &str = "marginalis-citation-style";

/// テンプレートノートを識別するタグ。
///
/// このタグを付けたノートは、新規作成の雛形として一覧に出す。専用の属性や設定を
/// 増やさず、通常のタグ運用の中でテンプレートを管理できるようにするための規約である。
pub const NOTE_TEMPLATE_TAG: &str = "テンプレート";

/// グラフビューで、起点から辿れる線の本数の上限。
///
/// 上限がないと、階層数の指定が全体表示と変わらなくなり、範囲を絞ろうとした利用者の意図と
/// 離れる。読み取りの入力規則であるため、ノート入力の規則とは別に置く。
pub const MAX_GRAPH_DEPTH: u32 = 5;

/// 現行のノート入力規則。
pub const NOTE_POLICY: NotePolicy = NotePolicy {
    max_title_characters: 200,
    max_source_bytes: 512 * 1024,
    max_patch_bytes: 512 * 1024,
    max_patch_hunks: 100,
    max_tags: 50,
    max_tag_characters: 64,
    max_output_bytes: 50 * 1024 * 1024,
    allowed_source_languages: &[
        "rust",
        "python",
        "typescript",
        "javascript",
        "json",
        "yaml",
        "toml",
        "bash",
        "sql",
        "text",
    ],
    allowed_math_languages: &["latexmath"],
    allowed_mathematical_block_roles: &[
        "definition",
        "proposition",
        "lemma",
        "theorem",
        "corollary",
        "proof",
    ],
    allowed_url_schemes: &["http", "https"],
    allowed_document_attributes: &[
        TAGS_DOCUMENT_ATTRIBUTE,
        CITATION_STYLE_DOCUMENT_ATTRIBUTE,
        "sectnums",
        "toc",
        "toclevels",
        "stem",
        "source-language",
    ],
    allowed_citation_styles: &["author-year", "numeric"],
};

impl NotePolicy {
    /// 題名が長すぎる場合の説明。
    pub fn invalid_title_message(&self) -> String {
        format!(
            "title must be non-empty, single-line, and at most {} characters",
            self.max_title_characters
        )
    }

    /// タグが規則に合わない場合の説明。
    pub fn invalid_tag_message(&self) -> String {
        format!(
            "tag must be non-empty, single-line, comma-free, and at most {} characters",
            self.max_tag_characters
        )
    }

    /// タグが多すぎる場合の説明。
    pub fn too_many_tags_message(&self) -> String {
        format!("a note may contain at most {} tags", self.max_tags)
    }

    /// 引用スタイルに選べない値を書いた場合の説明。
    ///
    /// 選べる値を並べて示します。任意のCSLスタイル名を受け付けないことが、拒否された利用者に
    /// 分かるようにするためです。
    pub fn unsupported_citation_style_message(&self) -> String {
        format!(
            "citation style must be one of: {}",
            self.allowed_citation_styles.join(", ")
        )
    }

    /// 文書が大きすぎる場合の説明。
    pub fn source_too_large_message(&self) -> String {
        format!(
            "source must be at most {} UTF-8 bytes",
            self.max_source_bytes
        )
    }

    /// patchが大きすぎる場合の説明。
    pub fn patch_too_large_message(&self) -> String {
        format!("patch must be at most {} UTF-8 bytes", self.max_patch_bytes)
    }

    /// patchのhunkが多すぎる場合の説明。
    pub fn too_many_patch_hunks_message(&self) -> String {
        format!("a patch may contain at most {} hunks", self.max_patch_hunks)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 説明文が上限値から生成され、値と食い違わないことを確認する。
    #[test]
    fn messages_are_generated_from_the_limits() {
        assert!(
            NOTE_POLICY
                .invalid_title_message()
                .contains(&NOTE_POLICY.max_title_characters.to_string())
        );
        assert!(
            NOTE_POLICY
                .invalid_tag_message()
                .contains(&NOTE_POLICY.max_tag_characters.to_string())
        );
        assert!(
            NOTE_POLICY
                .too_many_tags_message()
                .contains(&NOTE_POLICY.max_tags.to_string())
        );
        assert!(
            NOTE_POLICY
                .source_too_large_message()
                .contains(&NOTE_POLICY.max_source_bytes.to_string())
        );
        assert!(
            NOTE_POLICY
                .patch_too_large_message()
                .contains(&NOTE_POLICY.max_patch_bytes.to_string())
        );
        assert!(
            NOTE_POLICY
                .too_many_patch_hunks_message()
                .contains(&NOTE_POLICY.max_patch_hunks.to_string())
        );
    }

    #[test]
    fn allowed_sets_are_not_empty_and_have_no_duplicates() {
        for set in [
            NOTE_POLICY.allowed_source_languages,
            NOTE_POLICY.allowed_math_languages,
            NOTE_POLICY.allowed_url_schemes,
        ] {
            assert!(!set.is_empty());
            let mut sorted = set.to_vec();
            sorted.sort_unstable();
            let total = sorted.len();
            sorted.dedup();
            assert_eq!(sorted.len(), total, "許可集合に重複があります: {set:?}");
        }
    }
}
