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
    /// 本文に記述できるリンクのURLスキーム。
    pub allowed_url_schemes: &'static [&'static str],
    /// 文書headerへ書ける文書属性の名前。
    ///
    /// 入力検査と`get_note_profile`の広告は、どちらもこの一覧から導きます。片方だけを直すと、
    /// 受理する入力と公開する制約が食い違います。
    pub allowed_document_attributes: &'static [&'static str],
}

/// 関係の図で、起点から辿れる線の本数の上限。
///
/// 上限がないと、階層数の指定が全体表示と変わらなくなり、範囲を絞ろうとした利用者の意図と
/// 離れる。読み取りの入力規則であるため、ノート入力の規則とは別に置く。
pub const MAX_GRAPH_DEPTH: u32 = 5;

/// 現行のノート入力規則。
pub const NOTE_POLICY: NotePolicy = NotePolicy {
    max_title_characters: 200,
    max_source_bytes: 512 * 1024,
    max_tags: 50,
    max_tag_characters: 64,
    max_output_bytes: 50 * 1024 * 1024,
    allowed_source_languages: &[
        "rust",
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
    allowed_url_schemes: &["http", "https"],
    allowed_document_attributes: &[
        "tags",
        "sectnums",
        "toc",
        "toclevels",
        "stem",
        "source-language",
    ],
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

    /// 文書が大きすぎる場合の説明。
    pub fn source_too_large_message(&self) -> String {
        format!(
            "source must be at most {} UTF-8 bytes",
            self.max_source_bytes
        )
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
