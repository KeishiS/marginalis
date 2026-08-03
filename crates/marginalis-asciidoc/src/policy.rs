use std::collections::BTreeSet;

use adocweave::preprocess::discover_includes;
use adocweave::resolution::ReferenceKey;
use adocweave::semantic::{
    Block, DelimitedContent, Inline, MathLanguage, SemanticNode, VerbatimKind, walk,
};
use adocweave::text::TextRange;
use marginalis_application::{
    NoteAdvisoryDiagnostic, NoteAdvisorySeverity, NoteProfile, NoteProfileExample,
    NoteProfileLimits, NoteProfileNormalization, NoteProfileRule, NoteProfileSyntax,
    NoteValidationCode, NoteValidationDiagnostic,
};
use marginalis_domain::{NOTE_POLICY, NoteValidationTarget, Utf8ByteSpan};

use crate::{
    AUTHORING_PROFILE_VERSION, PINNED_ADOCWEAVE_PACKAGE_VERSION, configuration::authored_url_policy,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct NoteContentError {
    pub(crate) code: NoteValidationCode,
    pub(crate) range: TextRange,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ForbiddenRule {
    IncludeDirective,
    InlinePassthrough,
    BlockPassthrough,
    DuplicateAnchor,
    ExternalReference,
    InvalidUrlScheme,
    Resource,
    UnsupportedMathLanguage,
    UnsupportedSourceLanguage,
}

const FORBIDDEN_RULES: &[ForbiddenRule] = &[
    ForbiddenRule::IncludeDirective,
    ForbiddenRule::InlinePassthrough,
    ForbiddenRule::BlockPassthrough,
    ForbiddenRule::DuplicateAnchor,
    ForbiddenRule::ExternalReference,
    ForbiddenRule::InvalidUrlScheme,
    ForbiddenRule::Resource,
    ForbiddenRule::UnsupportedMathLanguage,
    ForbiddenRule::UnsupportedSourceLanguage,
];

impl ForbiddenRule {
    const fn code(self) -> NoteValidationCode {
        match self {
            Self::IncludeDirective => NoteValidationCode::IncludeDirectiveDisabled,
            Self::InlinePassthrough => NoteValidationCode::InlinePassthroughDisabled,
            Self::BlockPassthrough => NoteValidationCode::BlockPassthroughDisabled,
            Self::DuplicateAnchor => NoteValidationCode::DuplicateAnchor,
            Self::ExternalReference => NoteValidationCode::ExternalReferenceDisabled,
            Self::InvalidUrlScheme => NoteValidationCode::InvalidUrlScheme,
            Self::Resource => NoteValidationCode::ResourceDisabled,
            Self::UnsupportedMathLanguage => NoteValidationCode::UnsupportedMathLanguage,
            Self::UnsupportedSourceLanguage => NoteValidationCode::UnsupportedSourceLanguage,
        }
    }

    const fn message(self) -> &'static str {
        match self {
            Self::IncludeDirective => "include directives are not allowed",
            Self::InlinePassthrough => "inline passthrough is not allowed",
            Self::BlockPassthrough => "block passthrough is not allowed",
            Self::DuplicateAnchor => "anchor IDs must be unique within a note",
            Self::ExternalReference => "references other than the note scheme are not allowed",
            Self::InvalidUrlScheme => "the authored link target is not allowed",
            Self::Resource => "external media resources are not allowed",
            Self::UnsupportedMathLanguage => "only latexmath formulas are allowed",
            Self::UnsupportedSourceLanguage => "the source block language is not allowed",
        }
    }
}

impl NoteContentError {
    const fn forbidden(rule: ForbiddenRule, range: TextRange) -> Self {
        Self {
            code: rule.code(),
            range,
        }
    }
}

pub(crate) fn diagnostic(
    code: NoteValidationCode,
    target: NoteValidationTarget,
    span: Option<Utf8ByteSpan>,
) -> NoteValidationDiagnostic {
    NoteValidationDiagnostic {
        code: code.as_str().into(),
        target,
        span,
        message: diagnostic_message(code),
    }
}

pub(crate) fn advisory_diagnostic(
    code: &str,
    message: &str,
    severity: NoteAdvisorySeverity,
    target: NoteValidationTarget,
    span: Option<Utf8ByteSpan>,
) -> NoteAdvisoryDiagnostic {
    NoteAdvisoryDiagnostic {
        code: code.into(),
        severity,
        target,
        span,
        message: message.into(),
    }
}

/// 診断の説明文。上限値を含む文は[`NOTE_POLICY`]から生成し、値と食い違わせない。
fn diagnostic_message(code: NoteValidationCode) -> String {
    match code {
        NoteValidationCode::InvalidTitle => NOTE_POLICY.invalid_title_message(),
        NoteValidationCode::InvalidTag => NOTE_POLICY.invalid_tag_message(),
        NoteValidationCode::TooManyTags => NOTE_POLICY.too_many_tags_message(),
        NoteValidationCode::SourceTooLarge => NOTE_POLICY.source_too_large_message(),
        NoteValidationCode::AsciiDocParseFailed => "body is not valid AsciiDoc".to_owned(),
        NoteValidationCode::InvalidNoteReference => {
            "note reference locator must be a valid note ID".to_owned()
        }
        NoteValidationCode::UnsupportedDocumentAttribute => {
            "the document attribute is not allowed".to_owned()
        }
        NoteValidationCode::PreprocessorDirectiveDisabled => {
            "preprocessor directives such as include, ifdef, ifndef, and ifeval are not allowed"
                .to_owned()
        }
        NoteValidationCode::UnsupportedCitationStyle => {
            NOTE_POLICY.unsupported_citation_style_message()
        }
        forbidden => FORBIDDEN_RULES
            .iter()
            .find_map(|rule| (rule.code() == forbidden).then_some(rule.message()))
            .unwrap_or("note content is not allowed")
            .to_owned(),
    }
}

pub(crate) const fn span(range: TextRange) -> Utf8ByteSpan {
    Utf8ByteSpan {
        start: range.start().to_u32(),
        end: range.end().to_u32(),
    }
}

pub(crate) fn diagnostic_sort_key(
    target: &NoteValidationTarget,
    span: Option<Utf8ByteSpan>,
    code: &str,
) -> (u8, usize, u32, u32, String) {
    let (target, index) = match target {
        NoteValidationTarget::Source => (0, 0),
        NoteValidationTarget::Title => (1, 0),
        NoteValidationTarget::Tags => (2, 0),
        NoteValidationTarget::Tag { index } => (3, *index),
        NoteValidationTarget::Body => (4, 0),
        NoteValidationTarget::AclEntry { index } => (5, *index),
    };
    let span = span.unwrap_or(Utf8ByteSpan { start: 0, end: 0 });
    (target, index, span.start, span.end, code.to_owned())
}

/// 検証器と同じ正本から生成する機械可読なノート入力規則。
pub fn note_profile() -> NoteProfile {
    NoteProfile {
        profile_version: AUTHORING_PROFILE_VERSION,
        adocweave_package_version: PINNED_ADOCWEAVE_PACKAGE_VERSION,
        limits: NoteProfileLimits {
            max_title_characters: NOTE_POLICY.max_title_characters,
            max_source_bytes: NOTE_POLICY.max_source_bytes,
            max_tags: NOTE_POLICY.max_tags,
            max_tag_characters: NOTE_POLICY.max_tag_characters,
        },
        normalization: NoteProfileNormalization {
            title: vec!["trim", "unicode_nfc"],
            tags: vec![
                "trim",
                "unicode_nfc",
                "case_insensitive_uniqueness",
                "lowercase_key_sort",
            ],
        },
        syntax: NoteProfileSyntax {
            common_blocks: vec![
                "paragraph",
                "section",
                "list",
                "bibliography",
                "table",
                "admonition",
                "quote",
                "example",
                "literal",
                "source",
                "math",
            ],
            common_inlines: vec![
                "emphasis",
                "strong",
                "monospace",
                "local_anchor",
                "local_cross_reference",
                "bibliography_anchor",
                "bibliography_reference",
                "citation",
                "note_reference",
                "safe_link",
                "inline_math",
            ],
            source_language_optional: true,
            allowed_math_languages: NOTE_POLICY.allowed_math_languages.to_vec(),
            allowed_document_attributes: NOTE_POLICY.allowed_document_attributes.to_vec(),
            allowed_citation_styles: NOTE_POLICY.allowed_citation_styles.to_vec(),
            title_forbidden: vec!["empty", "line_feed", "carriage_return"],
            tag_forbidden: vec!["empty", "comma", "line_feed", "carriage_return"],
        },
        authoring_guidance: vec![
            "Use bibliographic metadata supplied by the user or an identified source. Never invent or infer authors, titles, publication years, DOIs, or other bibliographic metadata.",
            "A cite: macro names citation keys held by the bibliography library of the user who wrote the note. Register the item before citing it; an unregistered key is reported as a warning and is shown as the bare key.",
            "The reference list is built when the note is displayed, from the cited items only. Do not write a [bibliography] section for items that cite: already names.",
        ],
        allowed_source_languages: NOTE_POLICY.allowed_source_languages.to_vec(),
        forbidden_rules: FORBIDDEN_RULES
            .iter()
            .map(|rule| NoteProfileRule {
                code: rule.code(),
                description: rule.message(),
            })
            .collect(),
        examples: vec![
            NoteProfileExample {
                kind: "document_attributes",
                description: "Header attributes that control the rendered display",
                body: "= 調査の記録\n:marginalis-tags: 研究\n:sectnums:\n:toc:\n:toclevels: 2\n\n== 背景\n\n本文。\n\n== 方法\n\n本文。",
            },
            NoteProfileExample {
                kind: "local_reference",
                description: "Section, local anchor, and local cross-reference",
                body: "== Result\n\n[[evidence]]\nEvidence.\n\nSee <<evidence>>.",
            },
            NoteProfileExample {
                kind: "note_reference",
                description: "Reference to another note",
                body: "xref:note:0197c9bc-0000-7000-8000-000000000001[Related note]",
            },
            NoteProfileExample {
                kind: "multiline_list_item",
                description: "List item wrapped across source lines with an explicit hard break",
                body: "* First line +\nContinued line\n* Next item",
            },
            NoteProfileExample {
                kind: "source_block",
                description: "Titled Rust source block with line numbers starting at 7",
                body: ".Example\n[source,rust,linenums,start=7]\n----\nfn main() {}\n----",
            },
            NoteProfileExample {
                kind: "inline_math",
                description: "LaTeX math",
                body: ":stem: latexmath\n\nstem:[x^2 + y^2]",
            },
            NoteProfileExample {
                kind: "block_math",
                description: "LaTeX math block",
                body: "[latexmath]\n++++\nx^2 + y^2\n++++",
            },
            NoteProfileExample {
                kind: "bibliography",
                description: "Complete document with a bibliography entry and an in-text reference",
                body: "= 先行研究の整理\n:marginalis-tags: 文献, 研究\n\nSmithらは、対象の手法が有効だと報告しています <<smith2024>>。\n\n[bibliography]\n== 参考文献\n\n* [[[smith2024]]] Smith, A. et al. _Example Paper_. Example Journal, 2024.\n  https://doi.org/10.1234/replace-with-doi[DOI]",
            },
            NoteProfileExample {
                kind: "citation",
                description: "Citation of an item registered in the bibliography library, resolved when the note is displayed",
                body: "= 先行研究の整理\n:marginalis-tags: 文献, 研究\n\nこの手法は有効だと報告されています cite:[smith2024]。\n\n引用箇所を示す場合は cite:[smith2024, locator=\"p. 12\"] のように書きます。",
            },
            NoteProfileExample {
                kind: "citation-style",
                description: "Citations numbered in order of first appearance instead of author and year",
                body: "= 投稿原稿の下書き\n:marginalis-tags: 執筆\n:marginalis-citation-style: numeric\n\n結果は cite:[smith2024] で報告されています。\n\n追試も cite:[tanaka2025] で行われました。",
            },
        ],
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct NoteContentProfile {
    allowed_source_languages: BTreeSet<String>,
}

impl Default for NoteContentProfile {
    fn default() -> Self {
        Self {
            allowed_source_languages: NOTE_POLICY
                .allowed_source_languages
                .iter()
                .map(|language| (*language).to_owned())
                .collect(),
        }
    }
}

pub(crate) fn validate_note_content_profile(
    analysis: &adocweave::Analysis,
) -> Vec<NoteContentError> {
    validate_note_content_profile_with(analysis, &NoteContentProfile::default())
}

/// 指定したホスト側プロファイルで、I/O、raw HTMLおよび未許可の表示経路を検証する。
/// コードブロックの言語が許可集合に含まれるかを検査する。
///
/// `[source]`blockと`----`で囲むverbatim blockは、AdocWeaveの表現は異なるが同じ規則を適用する。
/// AdocWeaveでの表現が異なるため、共通する項目だけを受け取る。
fn unsupported_source_language(
    profile: &NoteContentProfile,
    language: Option<&str>,
    language_range: Option<TextRange>,
    attribute_range: TextRange,
) -> Option<NoteContentError> {
    let normalized = language?.to_ascii_lowercase();
    (!profile.allowed_source_languages.contains(&normalized)).then(|| {
        NoteContentError::forbidden(
            ForbiddenRule::UnsupportedSourceLanguage,
            language_range.unwrap_or(attribute_range),
        )
    })
}

fn validate_note_content_profile_with(
    analysis: &adocweave::Analysis,
    profile: &NoteContentProfile,
) -> Vec<NoteContentError> {
    let authored_url_policy = authored_url_policy();
    let mut errors = discover_includes(analysis.source())
        .expect("analysis source must have a representable byte length")
        .into_iter()
        .map(|request| NoteContentError::forbidden(ForbiddenRule::IncludeDirective, request.range))
        .collect::<Vec<_>>();
    errors.extend(analysis.resource_queries().into_iter().map(|query| {
        NoteContentError::forbidden(ForbiddenRule::Resource, query.reference.range())
    }));
    errors.extend(
        analysis
            .reference_queries()
            .into_iter()
            .filter(|query| match &query.target {
                ReferenceKey::Local { .. } => false,
                ReferenceKey::Scheme { scheme, .. } => scheme != "note",
                ReferenceKey::Document { .. } => true,
            })
            .map(|query| {
                NoteContentError::forbidden(ForbiddenRule::ExternalReference, query.source_range)
            }),
    );
    walk(analysis.document(), |node| match node {
        SemanticNode::Inline(Inline::Passthrough { range, .. }) => errors.push(
            NoteContentError::forbidden(ForbiddenRule::InlinePassthrough, *range),
        ),
        SemanticNode::Block(Block::Delimited(block))
            if matches!(block.content, DelimitedContent::Passthrough(_)) =>
        {
            errors.push(NoteContentError::forbidden(
                ForbiddenRule::BlockPassthrough,
                block.range,
            ));
        }
        SemanticNode::Inline(Inline::Formula(formula))
            if formula.language != MathLanguage::Latex =>
        {
            errors.push(NoteContentError::forbidden(
                ForbiddenRule::UnsupportedMathLanguage,
                formula.range,
            ));
        }
        SemanticNode::Block(Block::Math(math)) if math.language != MathLanguage::Latex => {
            errors.push(NoteContentError::forbidden(
                ForbiddenRule::UnsupportedMathLanguage,
                math.range,
            ));
        }
        SemanticNode::Block(Block::Source(source)) => {
            errors.extend(unsupported_source_language(
                profile,
                source.language.as_deref(),
                source.language_range,
                source.attribute_range,
            ));
        }
        SemanticNode::Block(Block::Verbatim(block)) => {
            if let VerbatimKind::Source(source) = &block.kind {
                errors.extend(unsupported_source_language(
                    profile,
                    source.language.as_deref(),
                    source.language_range,
                    source.attribute_range,
                ));
            }
        }
        SemanticNode::Inline(Inline::Link(link)) if !authored_url_policy.allows(&link.target) => {
            errors.push(NoteContentError::forbidden(
                ForbiddenRule::InvalidUrlScheme,
                link.target_range,
            ));
        }
        _ => {}
    });
    let mut seen_anchor_ids = BTreeSet::new();
    for target in analysis.reference_targets() {
        if !seen_anchor_ids.insert(&target.id) {
            errors.push(NoteContentError::forbidden(
                ForbiddenRule::DuplicateAnchor,
                target.id_range,
            ));
        }
    }
    errors.sort_by_key(|error| (error.range.start(), error.range.end(), error.code.as_str()));
    errors
}

#[cfg(test)]
mod tests {
    use marginalis_application::NoteValidationCode;

    use super::*;

    /// 公開している文書例を、そのまま保存できる完全な文書へ組み立てる。
    ///
    /// 断片の例は文書headerを持たない。文書属性はheaderの中だけで有効なため、先頭の
    /// 属性行を題名の直後へ移し、残りを本文として続ける。
    fn example_document(body: &str) -> String {
        if body.starts_with("= ") {
            return body.to_owned();
        }
        let attributes = body
            .lines()
            .take_while(|line| line.starts_with(':'))
            .collect::<Vec<_>>();
        let rest = body
            .lines()
            .skip(attributes.len())
            .skip_while(|line| line.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        format!("= 見出し\n{}\n\n{rest}", attributes.join("\n"))
    }

    /// 公開している文書例は、そのまま保存できる。
    ///
    /// MCPの書き込みは警告水準の診断があるとノートを変更しない。例を「この形なら受理される」
    /// ものとして公開している以上、公開した経路で通らない例を載せない。
    #[test]
    fn published_examples_are_accepted_without_advisories() {
        for example in note_profile().examples {
            let source = example_document(example.body);
            let validated = crate::analysis::validate_draft(marginalis_domain::NoteDraft {
                source,
                title: String::new(),
                tags: Vec::new(),
            })
            .unwrap_or_else(|errors| {
                panic!(
                    "{}の例が拒否されました: {:?}",
                    example.kind,
                    errors.iter().map(|error| &error.code).collect::<Vec<_>>()
                )
            });
            let advisories = validated
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.code.as_str())
                .collect::<Vec<_>>();
            assert!(
                advisories.is_empty(),
                "{}の例に診断が付きます: {advisories:?}",
                example.kind
            );
        }
    }

    /// 固定入力を解析し、禁止規則の検出結果をcodeの一覧として返す。
    ///
    /// `analyze_valid_source`は禁止規則を検出した時点で失敗するため、ここでは同じ設定で
    /// 解析だけを行い、規則の判定結果を取り出す。
    fn violations(source: &str) -> Vec<NoteValidationCode> {
        let analysis = adocweave::Engine::new(crate::configuration::analysis_options())
            .analyze(source)
            .expect("構文として解析できる入力");
        validate_note_content_profile(&analysis)
            .into_iter()
            .map(|error| error.code)
            .collect()
    }

    const HEADER: &str = "= 題名\n\n";

    #[test]
    fn accepts_content_that_satisfies_every_rule() {
        let source = format!(
            "{HEADER}本文です。\n\n\
             [source,rust]\n----\nfn main() {{}}\n----\n\n\
             link:https://example.test[例]\n\n\
             [[anchor-a]]\n== 節A\n\n\
             stem:[a + b]\n"
        );
        assert!(
            violations(&source).is_empty(),
            "許可した記法は拒否しません: {:?}",
            violations(&source)
        );
    }

    #[test]
    fn accepts_python_source_blocks() {
        let source = format!("{HEADER}[source,python]\n----\nprint(\"hello\")\n----\n");
        assert!(
            violations(&source).is_empty(),
            "Pythonのソースブロックを受理します: {:?}",
            violations(&source)
        );
    }

    #[test]
    fn rejects_source_languages_outside_the_policy() {
        let source = format!("{HEADER}[source,malbolge]\n----\nx\n----\n");
        assert!(
            violations(&source).contains(&NoteValidationCode::UnsupportedSourceLanguage),
            "許可していない言語を拒否します"
        );
    }

    /// `[source]`blockとverbatim blockで同じ規則が適用されることを確認する。
    ///
    /// 以前は二つの経路が別々に書かれており、片方だけを変更できる状態だった。
    #[test]
    fn applies_the_same_source_language_rule_to_both_block_forms() {
        for source in [
            // 単独のコードブロック
            format!("{HEADER}[source,malbolge]\n----\nx\n----\n"),
            // リスト継続の中のコードブロック
            format!("{HEADER}* 項目\n+\n[source,malbolge]\n----\nx\n----\n"),
        ] {
            assert!(
                violations(&source).contains(&NoteValidationCode::UnsupportedSourceLanguage),
                "どちらの記法でも同じ規則を適用します: {source}"
            );
        }
    }

    #[test]
    fn rejects_url_schemes_outside_the_policy() {
        let source = format!("{HEADER}link:javascript:alert(1)[危険]\n");
        assert!(violations(&source).contains(&NoteValidationCode::InvalidUrlScheme));
    }

    #[test]
    fn rejects_passthrough_and_include_and_resources() {
        let cases = [
            (
                format!("{HEADER}pass:[<script>x</script>]\n"),
                NoteValidationCode::InlinePassthroughDisabled,
            ),
            (
                format!("{HEADER}++++\n<script>x</script>\n++++\n"),
                NoteValidationCode::BlockPassthroughDisabled,
            ),
            (
                format!("{HEADER}include::other.adoc[]\n"),
                NoteValidationCode::IncludeDirectiveDisabled,
            ),
            (
                format!("{HEADER}image::https://example.test/a.png[]\n"),
                NoteValidationCode::ResourceDisabled,
            ),
        ];
        for (source, expected) in cases {
            assert!(
                violations(&source).contains(&expected),
                "{expected:?}を検出します: {source}"
            );
        }
    }

    #[test]
    fn rejects_duplicate_anchors() {
        let source = format!("{HEADER}[[same]]\n== 節A\n\n本文A\n\n[[same]]\n== 節B\n\n本文B\n");
        assert!(violations(&source).contains(&NoteValidationCode::DuplicateAnchor));
    }

    /// 書誌ライブラリーを参照する引用と、標準のbibliographyの両方を受理する。
    ///
    /// citation keyの解決は保存時ではなく描画時に行うため、入力規則としては引用の
    /// 書き方だけを見る。未登録のkeyは保存を妨げない警告として別に報告する。
    #[test]
    fn accepts_citations_and_the_standard_bibliography() {
        for source in [
            format!("{HEADER}本文 cite:[smith2024]。\n"),
            format!("{HEADER}本文 cite:[smith2024, tanaka2025]。\n"),
            format!("{HEADER}本文 cite:[smith2024, locator=\"p. 12\"]。\n"),
            format!(
                "{HEADER}本文<<smith2024>>。\n\n[bibliography]\n== 参考文献\n\n* [[[smith2024]]] Smith. Example.\n"
            ),
        ] {
            assert!(
                violations(&source).is_empty(),
                "引用を受理します: {source} {:?}",
                violations(&source)
            );
        }
    }

    #[test]
    fn rejects_references_outside_the_note_scheme() {
        let source = format!("{HEADER}xref:other:1234[別体系]\n");
        assert!(violations(&source).contains(&NoteValidationCode::ExternalReferenceDisabled));
    }

    /// 説明文が[`NOTE_POLICY`]の上限値から生成されることを確認する。
    #[test]
    fn diagnostic_messages_carry_the_configured_limits() {
        assert!(
            diagnostic_message(NoteValidationCode::SourceTooLarge)
                .contains(&NOTE_POLICY.max_source_bytes.to_string())
        );
        assert!(
            diagnostic_message(NoteValidationCode::TooManyTags)
                .contains(&NOTE_POLICY.max_tags.to_string())
        );
    }

    /// 公開するprofileが検証器と同じ正本から生成されることを確認する。
    #[test]
    fn profile_reports_the_same_policy_that_validation_applies() {
        let profile = note_profile();
        assert_eq!(
            profile.limits.max_source_bytes,
            NOTE_POLICY.max_source_bytes
        );
        assert_eq!(profile.limits.max_tags, NOTE_POLICY.max_tags);
        assert_eq!(
            profile.allowed_source_languages,
            NOTE_POLICY.allowed_source_languages.to_vec()
        );
        assert_eq!(
            profile.syntax.allowed_math_languages,
            NOTE_POLICY.allowed_math_languages.to_vec()
        );
        assert_eq!(profile.profile_version, AUTHORING_PROFILE_VERSION);
    }
}
