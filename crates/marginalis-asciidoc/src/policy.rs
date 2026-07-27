use std::collections::BTreeSet;

use adocweave::output::html::RenderPolicy;
use adocweave::preprocess::discover_includes;
use adocweave::resolution::{ReferenceKey, UrlContext};
use adocweave::semantic::{
    Block, DelimitedContent, Inline, MathLanguage, SemanticNode, VerbatimKind, walk,
};
use adocweave::text::TextRange;
use marginalis_application::{
    NoteProfile, NoteProfileExample, NoteProfileLimits, NoteProfileNormalization, NoteProfileRule,
    NoteProfileSyntax, NoteValidationCode, NoteValidationDiagnostic, NoteValidationTarget,
    Utf8ByteSpan,
};

use crate::{
    DEFAULT_SOURCE_LANGUAGES, MAX_NOTE_BODY_BYTES, MAX_TAG_CHARACTERS, MAX_TAGS,
    MAX_TITLE_CHARACTERS, NOTE_PROFILE_VERSION, PINNED_ADOCWEAVE_PACKAGE_VERSION,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct NoteContentError {
    pub(crate) code: NoteValidationCode,
    pub(crate) range: TextRange,
}

pub(crate) const FORBIDDEN_RULES: &[(NoteValidationCode, &str)] = &[
    (
        NoteValidationCode::IncludeDirectiveDisabled,
        "include directives are not allowed",
    ),
    (
        NoteValidationCode::InlinePassthroughDisabled,
        "inline passthrough is not allowed",
    ),
    (
        NoteValidationCode::BlockPassthroughDisabled,
        "block passthrough is not allowed",
    ),
    (
        NoteValidationCode::DuplicateAnchor,
        "anchor IDs must be unique within a note",
    ),
    (
        NoteValidationCode::ExternalReferenceDisabled,
        "cross-document and scheme references are not allowed",
    ),
    (
        NoteValidationCode::InvalidUrlScheme,
        "the authored link target is not allowed",
    ),
    (
        NoteValidationCode::ResourceDisabled,
        "external media resources are not allowed",
    ),
    (
        NoteValidationCode::UnsupportedSourceLanguage,
        "the source block language is not allowed",
    ),
];

pub(crate) fn diagnostic(
    code: NoteValidationCode,
    target: NoteValidationTarget,
    span: Option<Utf8ByteSpan>,
) -> NoteValidationDiagnostic {
    NoteValidationDiagnostic {
        code,
        target,
        span,
        message: diagnostic_message(code),
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
        NoteValidationCode::BodyTooLarge => "body must be at most 524288 UTF-8 bytes",
        NoteValidationCode::AsciiDocParseFailed => "body is not valid AsciiDoc",
        forbidden => FORBIDDEN_RULES
            .iter()
            .find_map(|(candidate, message)| (*candidate == forbidden).then_some(*message))
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
    diagnostic: &NoteValidationDiagnostic,
) -> (u8, usize, u32, u32, &'static str) {
    let (target, index) = match diagnostic.target {
        NoteValidationTarget::Title => (0, 0),
        NoteValidationTarget::Tags => (1, 0),
        NoteValidationTarget::Tag { index } => (2, index),
        NoteValidationTarget::Body => (3, 0),
    };
    let span = diagnostic.span.unwrap_or(Utf8ByteSpan { start: 0, end: 0 });
    (
        target,
        index,
        span.start,
        span.end,
        diagnostic.code.as_str(),
    )
}

/// 検証器と同じ正本から生成する機械可読なノート入力規則。
pub fn note_profile() -> NoteProfile {
    NoteProfile {
        profile_version: NOTE_PROFILE_VERSION,
        adocweave_package_version: PINNED_ADOCWEAVE_PACKAGE_VERSION,
        limits: NoteProfileLimits {
            max_title_characters: MAX_TITLE_CHARACTERS,
            max_body_bytes: MAX_NOTE_BODY_BYTES,
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
                "safe_link",
                "inline_math",
            ],
            source_language_optional: true,
            allowed_math_languages: vec!["latexmath"],
            title_forbidden: vec!["empty", "line_feed", "carriage_return"],
            tag_forbidden: vec!["empty", "comma", "line_feed", "carriage_return"],
        },
        allowed_source_languages: DEFAULT_SOURCE_LANGUAGES.to_vec(),
        forbidden_rules: FORBIDDEN_RULES
            .iter()
            .map(|(code, description)| NoteProfileRule {
                code: *code,
                description,
            })
            .collect(),
        examples: vec![
            NoteProfileExample {
                kind: "local_reference",
                description: "Section, local anchor, and local cross-reference",
                body: "== Result\n\n[[evidence]]\nEvidence.\n\nSee <<evidence>>.",
            },
            NoteProfileExample {
                kind: "source_block",
                description: "Rust source block",
                body: "[source,rust]\n----\nfn main() {}\n----",
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
    let render_policy = RenderPolicy::default();
    let mut errors = discover_includes(analysis.source())
        .expect("analysis source must have a representable byte length")
        .into_iter()
        .map(|request| NoteContentError {
            code: NoteValidationCode::IncludeDirectiveDisabled,
            range: request.range,
        })
        .collect::<Vec<_>>();
    errors.extend(
        analysis
            .resource_queries()
            .into_iter()
            .map(|query| NoteContentError {
                code: NoteValidationCode::ResourceDisabled,
                range: query.reference.range(),
            }),
    );
    errors.extend(
        analysis
            .reference_queries()
            .into_iter()
            .filter(|query| !matches!(query.target, ReferenceKey::Local { .. }))
            .map(|query| NoteContentError {
                code: NoteValidationCode::ExternalReferenceDisabled,
                range: query.source_range,
            }),
    );
    walk(analysis.document(), |node| match node {
        SemanticNode::Inline(Inline::Passthrough { range, .. }) => errors.push(NoteContentError {
            code: NoteValidationCode::InlinePassthroughDisabled,
            range: *range,
        }),
        SemanticNode::Block(Block::Delimited(block))
            if matches!(block.content, DelimitedContent::Passthrough(_)) =>
        {
            errors.push(NoteContentError {
                code: NoteValidationCode::BlockPassthroughDisabled,
                range: block.range,
            });
        }
        SemanticNode::Inline(Inline::Formula(formula))
            if formula.language != MathLanguage::Latex =>
        {
            errors.push(NoteContentError {
                code: NoteValidationCode::UnsupportedMathLanguage,
                range: formula.range,
            });
        }
        SemanticNode::Block(Block::Math(math)) if math.language != MathLanguage::Latex => {
            errors.push(NoteContentError {
                code: NoteValidationCode::UnsupportedMathLanguage,
                range: math.range,
            });
        }
        SemanticNode::Block(Block::Source(source)) => {
            let Some(language) = source.language.as_deref() else {
                return;
            };
            let normalized = language.to_ascii_lowercase();
            if !profile.allowed_source_languages.contains(&normalized) {
                errors.push(NoteContentError {
                    code: NoteValidationCode::UnsupportedSourceLanguage,
                    range: source.language_range.unwrap_or(source.attribute_range),
                });
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
                errors.push(NoteContentError {
                    code: NoteValidationCode::UnsupportedSourceLanguage,
                    range: source.language_range.unwrap_or(source.attribute_range),
                });
            }
        }
        SemanticNode::Inline(Inline::Link(link))
            if !render_policy.allows_url(&link.target, UrlContext::AuthoredLink) =>
        {
            errors.push(NoteContentError {
                code: NoteValidationCode::InvalidUrlScheme,
                range: link.target_range,
            });
        }
        _ => {}
    });
    let mut seen_anchor_ids = BTreeSet::new();
    for target in analysis.reference_targets() {
        if !seen_anchor_ids.insert(&target.id) {
            errors.push(NoteContentError {
                code: NoteValidationCode::DuplicateAnchor,
                range: target.id_range,
            });
        }
    }
    errors.sort_by_key(|error| (error.range.start(), error.range.end(), error.code.as_str()));
    errors
}
