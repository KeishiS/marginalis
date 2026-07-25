//! SQLite正本のAsciiDoc検証、可搬化、安全なHTML描画を担うadapter。

use core::fmt;
use std::collections::{BTreeMap, BTreeSet};

use adocweave::SyntaxMode;
use adocweave::output::html::{RenderPolicy, render};
use adocweave::preprocess::discover_includes;
use adocweave::resolution::UrlContext;
use adocweave::semantic::{
    Block, DelimitedContent, Inline, MathLanguage, SemanticNode, VerbatimKind, walk,
};
use adocweave::text::{TextRange, TextSize};
use marginalis_domain::{
    Note, NoteDraft, EntityId, NoteId, UnixMillis,
};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use unicode_normalization::UnicodeNormalization;

pub const ADOCWEAVE_SOURCE_REVISION: &str = "2a7ec4f7c2df6104ead9a7285ca13fc364ce8dda";

/// 初期リリースでシンタックスハイライト対象として受理するsource block言語。
pub const DEFAULT_SOURCE_LANGUAGES: &[&str] = &[
    "rust",
    "typescript",
    "javascript",
    "json",
    "yaml",
    "toml",
    "bash",
    "sql",
    "text",
];

/// 本アプリが受理するAdocWeaveの完全一致パッケージ版。
pub const PINNED_ADOCWEAVE_PACKAGE_VERSION: &str = "0.6.1";

/// 固定した仕様と実行時の仕様が異なる場合に返すエラー。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ContractMismatch {
    pub expected: &'static str,
    pub actual: &'static str,
}

impl fmt::Display for ContractMismatch {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "AdocWeave package version mismatch: expected {}, got {}",
            self.expected, self.actual
        )
    }
}

impl std::error::Error for ContractMismatch {}

/// リンクされた依存が、本アプリの固定したパッケージ版と一致することを検証する。
pub fn verify_runtime_package_version() -> Result<(), ContractMismatch> {
    let actual = adocweave::VERSION;
    if actual == PINNED_ADOCWEAVE_PACKAGE_VERSION {
        Ok(())
    } else {
        Err(ContractMismatch {
            expected: PINNED_ADOCWEAVE_PACKAGE_VERSION,
            actual,
        })
    }
}

/// SQLite正本から可搬用のAsciiDoc文書を生成できない理由。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExportError;

impl fmt::Display for ExportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("canonical note timestamp is outside the RFC 3339 range")
    }
}

impl std::error::Error for ExportError {}

/// 現行の単体AsciiDoc exportをimportできない理由。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImportError {
    InvalidDocument,
    InvalidNoteId,
    InvalidTimestamp,
    InvalidTags,
}

impl fmt::Display for ImportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidDocument => "canonical AsciiDoc export has invalid protected metadata",
            Self::InvalidNoteId => "canonical AsciiDoc export has an invalid note ID",
            Self::InvalidTimestamp => "canonical AsciiDoc export has an invalid timestamp",
            Self::InvalidTags => "canonical AsciiDoc export has invalid tags",
        })
    }
}

impl std::error::Error for ImportError {}

/// 現行のSQLite正本を単体export用のAsciiDocへ変換する。
///
/// headerは永続化しない。`note-id`、作成者、時刻、タグは正本から毎回生成するため、利用者が
/// server管理属性を偽装する経路を作らない。
pub fn export_note(note: &Note) -> Result<String, ExportError> {
    let created_at = format_unix_millis(note.created_at)?;
    let updated_at = format_unix_millis(note.updated_at)?;
    Ok(format!(
        "= {}\n:note-id: {}\n:creator-issuer: {}\n:creator-subject: {}\n:created-at: {}\n:updated-at: {}\n:tags: {}\n\n{}",
        note.title,
        note.note_id,
        note.creator_issuer,
        note.creator_subject,
        created_at,
        updated_at,
        note.tags.join(","),
        note.body,
    ))
}

/// SQLite正本を再検証した上で、固定RenderPolicyの安全なHTMLへ変換する。
pub fn render_note_html(note: &Note) -> Result<String, RenderError> {
    let source = export_note(note).map_err(|_| RenderError)?;
    let analysis = adocweave::Engine::new(adocweave::ParseOptions {
        syntax_mode: SyntaxMode::Strict,
        ..Default::default()
    })
    .analyze(&source)
    .map_err(|_| RenderError)?;
    if !validate_note_content_profile(&analysis).is_empty() {
        return Err(RenderError);
    }
    Ok(render(analysis.document(), &RenderPolicy::default()).html)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RenderError;

impl fmt::Display for RenderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("canonical note cannot be rendered safely")
    }
}

impl std::error::Error for RenderError {}

/// 単体exportを、空のSQLiteへimportできる構造化正本へ変換する。
///
/// exportには楽観的ロック世代と削除状態を含めないため、import結果は revision 1 の非削除ノートに
/// なる。ACL と削除状態を含む完全な復元には archive import を使う。
pub fn import_note(source: &str) -> Result<Note, ImportError> {
    let (header, body) = source
        .split_once("\n\n")
        .ok_or(ImportError::InvalidDocument)?;
    let mut lines = header.lines();
    let title = lines
        .next()
        .and_then(|line| line.strip_prefix("= "))
        .filter(|title| !title.is_empty() && !title.contains(['\r', '\n']))
        .ok_or(ImportError::InvalidDocument)?
        .to_owned();
    let mut attributes = BTreeMap::new();
    for line in lines {
        let (name, value) = line
            .strip_prefix(':')
            .and_then(|line| line.split_once(":"))
            .ok_or(ImportError::InvalidDocument)?;
        if name.is_empty()
            || (value.is_empty() && name != "tags")
            || attributes
                .insert(name.to_owned(), value.trim_start().to_owned())
                .is_some()
        {
            return Err(ImportError::InvalidDocument);
        }
    }
    let required = |name| {
        attributes
            .get(name)
            .filter(|value| !value.is_empty())
            .cloned()
            .ok_or(ImportError::InvalidDocument)
    };
    let note_id = required("note-id")?
        .parse::<EntityId>()
        .map(NoteId::new)
        .map_err(|_| ImportError::InvalidNoteId)?;
    let creator_issuer = required("creator-issuer")?;
    let creator_subject = required("creator-subject")?;
    let created_at = parse_unix_millis(&required("created-at")?)?;
    let updated_at = parse_unix_millis(&required("updated-at")?)?;
    let tags = parse_export_tags(
        attributes
            .get("tags")
            .ok_or(ImportError::InvalidDocument)?,
    )?;
    Ok(Note {
        note_id,
        creator_issuer,
        creator_subject,
        title,
        body: body.to_owned(),
        tags,
        created_at,
        updated_at,
        revision: 1,
        deleted_at: None,
    })
}

/// 現行の構造化入力を検証し、タグを正規化する。
///
/// SQLite正本ではheaderを保存しないため、従来の「文書全体の必須属性」検査とは分離して、
/// 利用者が送るtitle・tags・bodyだけを検査する。本文の位置は入力されたbodyを基準に返す。
pub fn validate_note_draft(
    draft: NoteDraft,
) -> Result<NoteDraft, Vec<NoteValidationError>> {
    let empty_range = TextRange::new(TextSize::ZERO, TextSize::ZERO).expect("empty range is valid");
    let mut errors = Vec::new();
    if draft.title.trim().is_empty()
        || draft.title.contains(['\n', '\r'])
        || draft.title.chars().count() > 200
    {
        errors.push(NoteValidationError {
            code: "invalid-title".into(),
            range: empty_range,
        });
    }
    let mut tags = BTreeMap::new();
    for tag in draft.tags {
        let display = tag.trim().nfc().collect::<String>();
        if display.is_empty() || display.contains([',', '\n', '\r']) || display.chars().count() > 64
        {
            errors.push(NoteValidationError {
                code: "invalid-tags".into(),
                range: empty_range,
            });
            continue;
        }
        tags.entry(display.to_lowercase()).or_insert(display);
    }
    if tags.len() > 50 {
        errors.push(NoteValidationError {
            code: "too-many-tags".into(),
            range: empty_range,
        });
    }
    let analysis = adocweave::Engine::new(Default::default())
        .analyze(&draft.body)
        .map_err(|_| {
            vec![NoteValidationError {
                code: "asciidoc-parse-failed".into(),
                range: empty_range,
            }]
        })?;
    errors.extend(
        validate_note_content_profile(&analysis)
            .into_iter()
            .map(|error| NoteValidationError {
                code: error.code.as_str().into(),
                range: error.range,
            }),
    );
    if errors.is_empty() {
        Ok(NoteDraft {
            title: draft.title.trim().nfc().collect(),
            body: draft.body,
            tags: tags.into_values().collect(),
        })
    } else {
        errors.sort_by_key(|error| (error.range.start(), error.range.end(), error.code.clone()));
        Err(errors)
    }
}

fn format_unix_millis(value: UnixMillis) -> Result<String, ExportError> {
    let nanos = i128::from(value.get()) * 1_000_000;
    OffsetDateTime::from_unix_timestamp_nanos(nanos)
        .map_err(|_| ExportError)?
        .format(&Rfc3339)
        .map_err(|_| ExportError)
}

fn parse_unix_millis(value: &str) -> Result<UnixMillis, ImportError> {
    let value = OffsetDateTime::parse(value, &Rfc3339)
        .map_err(|_| ImportError::InvalidTimestamp)?;
    i64::try_from(value.unix_timestamp_nanos() / 1_000_000)
        .map(UnixMillis::new)
        .map_err(|_| ImportError::InvalidTimestamp)
}

fn parse_export_tags(value: &str) -> Result<Vec<String>, ImportError> {
    if value.is_empty() {
        return Ok(Vec::new());
    }
    let mut normalized = BTreeMap::new();
    for value in value.split(',') {
        let display = value.trim().nfc().collect::<String>();
        if display.is_empty() || display.contains(['\n', '\r']) || display.chars().count() > 64 {
            return Err(ImportError::InvalidTags);
        }
        normalized.entry(display.to_lowercase()).or_insert(display);
    }
    if normalized.len() > 50 {
        return Err(ImportError::InvalidTags);
    }
    Ok(normalized.into_values().collect())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoteValidationError {
    pub code: String,
    pub range: TextRange,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct NoteContentError {
    code: NoteContentErrorCode,
    range: TextRange,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NoteContentErrorCode {
    IncludeDirective,
    InlinePassthrough,
    BlockPassthrough,
    DuplicateAnchor,
    InvalidUrlScheme,
    ResourceDisabled,
    UnsupportedMathLanguage,
    UnsupportedSourceLanguage,
}

impl NoteContentErrorCode {
    const fn as_str(self) -> &'static str {
        match self {
            Self::IncludeDirective => "include-directive-disabled",
            Self::InlinePassthrough => "inline-passthrough-disabled",
            Self::BlockPassthrough => "block-passthrough-disabled",
            Self::DuplicateAnchor => "duplicate-anchor",
            Self::InvalidUrlScheme => "invalid-url-scheme",
            Self::ResourceDisabled => "resource-disabled",
            Self::UnsupportedMathLanguage => "unsupported-math-language",
            Self::UnsupportedSourceLanguage => "unsupported-source-language",
        }
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

fn validate_note_content_profile(analysis: &adocweave::Analysis) -> Vec<NoteContentError> {
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
            code: NoteContentErrorCode::IncludeDirective,
            range: request.range,
        })
        .collect::<Vec<_>>();
    errors.extend(
        analysis
            .resource_queries()
            .into_iter()
            .map(|query| NoteContentError {
                code: NoteContentErrorCode::ResourceDisabled,
                range: query.reference.range,
            }),
    );
    walk(analysis.document(), |node| match node {
        SemanticNode::Inline(Inline::Passthrough { range, .. }) => errors.push(NoteContentError {
            code: NoteContentErrorCode::InlinePassthrough,
            range: *range,
        }),
        SemanticNode::Block(Block::Delimited(block))
            if matches!(block.content, DelimitedContent::Passthrough(_)) =>
        {
            errors.push(NoteContentError {
                code: NoteContentErrorCode::BlockPassthrough,
                range: block.range,
            });
        }
        SemanticNode::Inline(Inline::Formula(formula))
            if formula.language != MathLanguage::Latex =>
        {
            errors.push(NoteContentError {
                code: NoteContentErrorCode::UnsupportedMathLanguage,
                range: formula.range,
            });
        }
        SemanticNode::Block(Block::Math(math)) if math.language != MathLanguage::Latex => {
            errors.push(NoteContentError {
                code: NoteContentErrorCode::UnsupportedMathLanguage,
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
                    code: NoteContentErrorCode::UnsupportedSourceLanguage,
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
                    code: NoteContentErrorCode::UnsupportedSourceLanguage,
                    range: source.language_range.unwrap_or(source.attribute_range),
                });
            }
        }
        SemanticNode::Inline(Inline::Link(link))
            if !render_policy.allows_url(&link.target, UrlContext::AuthoredLink) =>
        {
            errors.push(NoteContentError {
                code: NoteContentErrorCode::InvalidUrlScheme,
                range: link.target_range,
            });
        }
        _ => {}
    });
    let mut seen_anchor_ids = BTreeSet::new();
    for target in analysis.reference_targets() {
        if !seen_anchor_ids.insert(&target.id) {
            errors.push(NoteContentError {
                code: NoteContentErrorCode::DuplicateAnchor,
                range: target.id_range,
            });
        }
    }
    errors.sort_by_key(|error| (error.range.start(), error.range.end(), error.code.as_str()));
    errors
}


#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;

    fn note(body: &str) -> Note {
        Note {
            note_id: NoteId::new(
                EntityId::from_str("0197c9bc-0000-7000-8000-000000000001")
                    .expect("UUIDv7"),
            ),
            creator_issuer: "https://id.example.test".into(),
            creator_subject: "alice".into(),
            title: "A title".into(),
            body: body.into(),
            tags: vec!["Research".into()],
            created_at: UnixMillis::new(0),
            updated_at: UnixMillis::new(1_000),
            revision: 1,
            deleted_at: None,
        }
    }

    #[test]
    fn package_version_matches_the_pinned_contract() {
        assert_eq!(ADOCWEAVE_SOURCE_REVISION.len(), 40);
        verify_runtime_package_version().expect("pinned version");
    }

    #[test]
    fn canonical_export_round_trips_without_acl_or_revision_state() {
        let exported = export_note(&note("body")).expect("export");
        let imported = import_note(&exported).expect("import");
        assert_eq!(imported.title, "A title");
        assert_eq!(imported.body, "body");
        assert_eq!(imported.revision, 1);
        assert_eq!(imported.deleted_at, None);
    }

    #[test]
    fn draft_validation_normalizes_tags() {
        let draft = validate_note_draft(NoteDraft {
            title: "  Title  ".into(),
            body: "safe body".into(),
            tags: vec![" Rust ".into(), "rust".into()],
        })
        .expect("valid draft");
        assert_eq!(draft.title, "Title");
        assert_eq!(draft.tags, vec!["Rust"]);
    }

    #[test]
    fn unsafe_content_is_rejected_before_rendering() {
        for body in [
            "include::secret[]",
            "pass:[<script>alert(1)</script>]",
            "link:javascript:alert(1)[unsafe]",
            "image::https://example.test/a.png[]",
        ] {
            assert!(
                validate_note_draft(NoteDraft {
                    title: "Title".into(),
                    body: body.into(),
                    tags: Vec::new(),
                })
                .is_err(),
                "{body} must be rejected"
            );
            assert_eq!(render_note_html(&note(body)), Err(RenderError));
        }
    }

    #[test]
    fn safe_content_renders_without_raw_markup() {
        let html = render_note_html(&note("A *safe* paragraph.")).expect("render");
        assert!(html.contains("<strong>safe</strong>"));
    }
}

