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
    NoteValidationCode, NoteValidationDiagnostic, NoteValidationTarget, Utf8ByteSpan,
};

use crate::{
    AUTHORING_PROFILE_VERSION, DEFAULT_SOURCE_LANGUAGES, MAX_NOTE_SOURCE_BYTES, MAX_TAG_CHARACTERS,
    MAX_TAGS, MAX_TITLE_CHARACTERS, PINNED_ADOCWEAVE_PACKAGE_VERSION,
    configuration::authored_url_policy,
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
        message: diagnostic_message(code).into(),
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

fn diagnostic_message(code: NoteValidationCode) -> &'static str {
    match code {
        NoteValidationCode::InvalidTitle => {
            "title must be non-empty, single-line, and at most 200 characters"
        }
        NoteValidationCode::InvalidTag => {
            "tag must be non-empty, single-line, comma-free, and at most 64 characters"
        }
        NoteValidationCode::TooManyTags => "a note may contain at most 50 tags",
        NoteValidationCode::SourceTooLarge => "source must be at most 524288 UTF-8 bytes",
        NoteValidationCode::AsciiDocParseFailed => "body is not valid AsciiDoc",
        NoteValidationCode::InvalidNoteReference => {
            "note reference locator must be a valid note ID"
        }
        NoteValidationCode::UnsupportedDocumentAttribute => "the document attribute is not allowed",
        forbidden => FORBIDDEN_RULES
            .iter()
            .find_map(|rule| (rule.code() == forbidden).then_some(rule.message()))
            .unwrap_or("note content is not allowed"),
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
            max_title_characters: MAX_TITLE_CHARACTERS,
            max_source_bytes: MAX_NOTE_SOURCE_BYTES,
            max_tags: MAX_TAGS,
            max_tag_characters: MAX_TAG_CHARACTERS,
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
                "note_reference",
                "safe_link",
                "inline_math",
            ],
            source_language_optional: true,
            allowed_math_languages: vec!["latexmath"],
            title_forbidden: vec!["empty", "line_feed", "carriage_return"],
            tag_forbidden: vec!["empty", "comma", "line_feed", "carriage_return"],
        },
        authoring_guidance: vec![
            "Use bibliographic metadata supplied by the user or an identified source. Never invent or infer authors, titles, publication years, DOIs, or other bibliographic metadata.",
        ],
        allowed_source_languages: DEFAULT_SOURCE_LANGUAGES.to_vec(),
        forbidden_rules: FORBIDDEN_RULES
            .iter()
            .map(|rule| NoteProfileRule {
                code: rule.code(),
                description: rule.message(),
            })
            .collect(),
        examples: vec![
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
                body: "= 先行研究の整理\n:tags: 文献, 研究\n\nSmithらは、対象の手法が有効だと報告しています <<smith2024>>。\n\n[bibliography]\n== 参考文献\n\n* [[[smith2024]]] Smith, A. et al. _Example Paper_. Example Journal, 2024. https://doi.org/10.1234/replace-with-doi[DOI]",
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
            allowed_source_languages: DEFAULT_SOURCE_LANGUAGES
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
            let Some(language) = source.language.as_deref() else {
                return;
            };
            let normalized = language.to_ascii_lowercase();
            if !profile.allowed_source_languages.contains(&normalized) {
                errors.push(NoteContentError::forbidden(
                    ForbiddenRule::UnsupportedSourceLanguage,
                    source.language_range.unwrap_or(source.attribute_range),
                ));
            }
        }
        SemanticNode::Block(Block::Verbatim(block)) => {
            let VerbatimKind::Source(source) = &block.kind else {
                return;
            };
            let Some(language) = source.language.as_deref() else {
                return;
            };
            let normalized = language.to_ascii_lowercase();
            if !profile.allowed_source_languages.contains(&normalized) {
                errors.push(NoteContentError::forbidden(
                    ForbiddenRule::UnsupportedSourceLanguage,
                    source.language_range.unwrap_or(source.attribute_range),
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
