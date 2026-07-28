//! SQLite正本のAsciiDoc検証、可搬化、安全なHTML描画を担うadapter。

use core::fmt;
use std::collections::BTreeMap;

#[cfg(test)]
use adocweave::OutputLimits;
use adocweave::output::diagnostics::Severity;
use adocweave::output::html::render_with_inputs;
use adocweave::resolution::{
    ReferenceKey, RenderInputs, ResolutionFailureKind, ResolutionNotice, ResolutionNoticeKind,
    ResolvedReference, ResolverFailure,
};
#[cfg(test)]
use marginalis_application::Utf8ByteSpan;
use marginalis_application::{
    NoteContent, NoteContentError, NoteProfile, NoteValidationCode, NoteValidationDiagnostic,
    NoteValidationTarget,
};
use marginalis_domain::{ARCHIVE_FORMAT, Archive, Note, NoteDraft, UnixMillis, validate_identity};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use unicode_normalization::UnicodeNormalization;

mod configuration;
mod policy;

use configuration::{
    analysis_options as note_analysis_options, html_is_within_output_limits,
    output_limits as note_output_limits, render_policy as note_render_policy,
};
pub use policy::note_profile;
use policy::{diagnostic, diagnostic_sort_key, span, validate_note_content_profile};

pub const ADOCWEAVE_SOURCE_REVISION: &str = "778e9da4548f03ea8434677d50c819d7ce665809";

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
pub const PINNED_ADOCWEAVE_PACKAGE_VERSION: &str = "0.11.0";
/// Marginalisが保存時に適用するノート入力規則の版。
pub const NOTE_PROFILE_VERSION: u32 = 2;
pub const MAX_TITLE_CHARACTERS: usize = 200;
pub const MAX_NOTE_BODY_BYTES: usize = 512 * 1024;
pub const MAX_TAGS: usize = 50;
pub const MAX_TAG_CHARACTERS: usize = 64;

/// 固定したAsciiDoc profileをノートapplicationへ接続するadapter。
#[derive(Clone, Copy, Debug, Default)]
pub struct AsciiDocNoteContent;

impl NoteContent for AsciiDocNoteContent {
    fn validate_draft(&self, draft: NoteDraft) -> Result<NoteDraft, Vec<NoteValidationDiagnostic>> {
        validate_note_draft(draft)
    }

    fn reference_queries(
        &self,
        body: &str,
    ) -> Result<Vec<marginalis_application::NoteReferenceQuery>, NoteContentError> {
        note_reference_queries(body)
            .map_err(|_| NoteContentError)
            .map(|queries| {
                queries
                    .into_iter()
                    .map(|query| marginalis_application::NoteReferenceQuery {
                        reference_index: query.reference_index,
                        target_note_id: query.target_note_id,
                        anchor: query.anchor,
                    })
                    .collect()
            })
    }

    fn has_anchor(&self, body: &str, anchor: &str) -> Result<bool, NoteContentError> {
        note_has_anchor(body, anchor).map_err(|_| NoteContentError)
    }

    fn render(
        &self,
        note: &Note,
        resolutions: &[marginalis_application::NoteReferenceResolution],
    ) -> Result<String, NoteContentError> {
        let resolutions = resolutions
            .iter()
            .map(|resolution| match resolution {
                marginalis_application::NoteReferenceResolution::Visible {
                    reference_index,
                    href,
                    title,
                    missing_anchor,
                } => NoteReferenceResolution::Visible {
                    reference_index: *reference_index,
                    href: href.clone(),
                    title: title.clone(),
                    missing_anchor: *missing_anchor,
                },
                marginalis_application::NoteReferenceResolution::Hidden { reference_index } => {
                    NoteReferenceResolution::Hidden {
                        reference_index: *reference_index,
                    }
                }
            })
            .collect::<Vec<_>>();
        render_note_html_with_references(note, &resolutions).map_err(|_| NoteContentError)
    }

    fn export(&self, note: &Note) -> Result<String, NoteContentError> {
        export_note(note).map_err(|_| NoteContentError)
    }

    fn profile(&self) -> NoteProfile {
        note_profile()
    }
}

/// ホストがACL判定するノート参照。順序は文書内の出現順です。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoteReferenceQuery {
    pub reference_index: usize,
    pub target_note_id: marginalis_domain::NoteId,
    pub anchor: Option<String>,
}

/// ホストが確定したノート参照の表示結果。
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

/// 固定した仕様と実行時の仕様が異なる場合に返すエラー。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PackageVersionMismatch {
    pub expected: &'static str,
    pub actual: &'static str,
}

impl fmt::Display for PackageVersionMismatch {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "AdocWeave package version mismatch: expected {}, got {}",
            self.expected, self.actual
        )
    }
}

impl std::error::Error for PackageVersionMismatch {}

/// リンクされた依存が、本アプリの固定したパッケージ版と一致することを検証する。
pub fn verify_runtime_package_version() -> Result<(), PackageVersionMismatch> {
    let actual = adocweave::VERSION;
    if actual == PINNED_ADOCWEAVE_PACKAGE_VERSION {
        Ok(())
    } else {
        Err(PackageVersionMismatch {
            expected: PINNED_ADOCWEAVE_PACKAGE_VERSION,
            actual,
        })
    }
}

/// SQLite正本から可搬用のAsciiDoc文書を生成できない理由。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExportError {
    InvalidIdentity,
    InvalidTimestamp,
}

impl fmt::Display for ExportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidIdentity => formatter.write_str("canonical note identity is invalid"),
            Self::InvalidTimestamp => {
                formatter.write_str("canonical note timestamp is outside the RFC 3339 range")
            }
        }
    }
}

impl std::error::Error for ExportError {}

/// 現行のSQLite正本を単体export用のAsciiDocへ変換する。
///
/// headerは永続化しない。`note-id`、作成者、時刻、タグは正本から毎回生成するため、利用者が
/// server管理属性を偽装する経路を作らない。
pub fn export_note(note: &Note) -> Result<String, ExportError> {
    validate_identity(note.creator_issuer(), note.creator_subject())
        .map_err(|_| ExportError::InvalidIdentity)?;
    let created_at = format_unix_millis(note.created_at())?;
    let updated_at = format_unix_millis(note.updated_at())?;
    Ok(format!(
        "= {}\n:note-id: {}\n:creator-issuer: {}\n:creator-subject: {}\n:created-at: {}\n:updated-at: {}\n:tags: {}\n\n{}",
        note.title(),
        note.note_id(),
        note.creator_issuer(),
        note.creator_subject(),
        created_at,
        updated_at,
        note.tags().join(","),
        note.body(),
    ))
}

/// SQLite正本を再検証した上で、固定RenderPolicyの安全なHTMLへ変換する。
pub fn render_note_html(note: &Note) -> Result<String, RenderError> {
    render_note_html_with_references(note, &[])
}

/// ノート参照を抽出し、ホスト側でACL判定するための問い合わせを返す。
pub fn note_reference_queries(body: &str) -> Result<Vec<NoteReferenceQuery>, RenderError> {
    let analysis = adocweave::Engine::new(note_analysis_options())
        .analyze(body)
        .map_err(|_| RenderError)?;
    if analysis
        .diagnostics()
        .iter()
        .any(|diagnostic| diagnostic.severity == Severity::Error)
        || !validate_note_content_profile(&analysis).is_empty()
    {
        return Err(RenderError);
    }
    analysis
        .reference_queries()
        .into_iter()
        .enumerate()
        .filter_map(|(reference_index, query)| match query.target {
            ReferenceKey::Scheme {
                scheme,
                locator,
                anchor,
            } if scheme == "note" => Some(
                locator
                    .parse::<marginalis_domain::EntityId>()
                    .map(|id| NoteReferenceQuery {
                        reference_index,
                        target_note_id: marginalis_domain::NoteId::new(id),
                        anchor,
                    })
                    .map_err(|_| RenderError),
            ),
            _ => None,
        })
        .collect()
}

/// 指定したanchorが対象ノートの参照先として存在するかを返す。
pub fn note_has_anchor(body: &str, anchor: &str) -> Result<bool, RenderError> {
    let analysis = adocweave::Engine::new(note_analysis_options())
        .analyze(body)
        .map_err(|_| RenderError)?;
    if analysis
        .diagnostics()
        .iter()
        .any(|diagnostic| diagnostic.severity == Severity::Error)
        || !validate_note_content_profile(&analysis).is_empty()
    {
        return Err(RenderError);
    }
    Ok(analysis
        .reference_targets()
        .iter()
        .any(|target| target.id == anchor))
}

/// ACL判定済みの参照だけを安全なHTMLへ反映する。
pub fn render_note_html_with_references(
    note: &Note,
    resolutions: &[NoteReferenceResolution],
) -> Result<String, RenderError> {
    let source = export_note(note).map_err(|_| RenderError)?;
    let analysis = adocweave::Engine::new(note_analysis_options())
        .analyze(&source)
        .map_err(|_| RenderError)?;
    if analysis
        .diagnostics()
        .iter()
        .any(|diagnostic| diagnostic.severity == Severity::Error)
        || !validate_note_content_profile(&analysis).is_empty()
    {
        return Err(RenderError);
    }
    let queries = analysis.reference_queries();
    let references = resolutions
        .iter()
        .map(|resolution| {
            let reference_index = match resolution {
                NoteReferenceResolution::Visible {
                    reference_index, ..
                }
                | NoteReferenceResolution::Hidden { reference_index } => *reference_index,
            };
            let query = queries.get(reference_index).ok_or(RenderError)?;
            Ok(match resolution {
                NoteReferenceResolution::Visible {
                    href,
                    title,
                    missing_anchor,
                    ..
                } => {
                    let resolved = ResolvedReference::resolved(query.source_range, href)
                        .with_display_text(title);
                    if *missing_anchor {
                        resolved.with_notices(vec![ResolutionNotice {
                            kind: ResolutionNoticeKind::Fallback,
                        }])
                    } else {
                        resolved
                    }
                }
                NoteReferenceResolution::Hidden { .. } => ResolvedReference::failed(
                    query.source_range,
                    ResolverFailure {
                        kind: ResolutionFailureKind::MissingTarget,
                    },
                ),
            })
        })
        .collect::<Result<Vec<_>, RenderError>>()?;
    let inputs = RenderInputs::new(references, Vec::new());
    let output = render_with_inputs(analysis.document(), &note_render_policy(), &inputs);
    if output
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == Severity::Error)
        || !html_is_within_output_limits(&output.html, &note_output_limits())
    {
        return Err(RenderError);
    }
    Ok(output.html)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RenderError;

impl fmt::Display for RenderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("canonical note cannot be rendered safely")
    }
}

impl std::error::Error for RenderError {}

/// 現行の構造化入力を検証し、タグを正規化する。
///
/// SQLite正本ではheaderを保存しないため、従来の「文書全体の必須属性」検査とは分離して、
/// 利用者が送るtitle・tags・bodyだけを検査する。本文の位置は入力されたbodyを基準に返す。
pub fn validate_note_draft(draft: NoteDraft) -> Result<NoteDraft, Vec<NoteValidationDiagnostic>> {
    let mut errors = Vec::new();
    let title = draft.title.trim().nfc().collect::<String>();
    if title.is_empty()
        || title.contains(['\n', '\r'])
        || title.chars().count() > MAX_TITLE_CHARACTERS
    {
        errors.push(diagnostic(
            NoteValidationCode::InvalidTitle,
            NoteValidationTarget::Title,
            None,
        ));
    }
    if draft.tags.len() > MAX_TAGS {
        errors.push(diagnostic(
            NoteValidationCode::TooManyTags,
            NoteValidationTarget::Tags,
            None,
        ));
    }
    let mut tags = BTreeMap::new();
    for (index, tag) in draft.tags.into_iter().enumerate() {
        let display = tag.trim().nfc().collect::<String>();
        if display.is_empty()
            || display.contains([',', '\n', '\r'])
            || display.chars().count() > MAX_TAG_CHARACTERS
        {
            errors.push(diagnostic(
                NoteValidationCode::InvalidTag,
                NoteValidationTarget::Tag { index },
                None,
            ));
            continue;
        }
        tags.entry(display.to_lowercase()).or_insert(display);
    }
    if draft.body.len() > MAX_NOTE_BODY_BYTES {
        errors.push(diagnostic(
            NoteValidationCode::BodyTooLarge,
            NoteValidationTarget::Body,
            None,
        ));
    } else {
        match adocweave::Engine::new(note_analysis_options()).analyze(&draft.body) {
            Ok(analysis) => {
                errors.extend(
                    analysis
                        .diagnostics()
                        .iter()
                        .filter(|diagnostic| diagnostic.severity == Severity::Error)
                        .map(|adoc_diagnostic| {
                            diagnostic(
                                NoteValidationCode::AsciiDocParseFailed,
                                NoteValidationTarget::Body,
                                Some(span(adoc_diagnostic.range)),
                            )
                        }),
                );
                errors.extend(
                    validate_note_content_profile(&analysis)
                        .into_iter()
                        .map(|error| {
                            diagnostic(
                                error.code,
                                NoteValidationTarget::Body,
                                Some(span(error.range)),
                            )
                        }),
                );
            }
            Err(_) => errors.push(diagnostic(
                NoteValidationCode::AsciiDocParseFailed,
                NoteValidationTarget::Body,
                None,
            )),
        }
    }
    if errors.is_empty() {
        Ok(NoteDraft {
            title,
            body: draft.body,
            tags: tags.into_values().collect(),
        })
    } else {
        errors.sort_by_key(diagnostic_sort_key);
        Err(errors)
    }
}

/// SQLiteの論理snapshotへ現行のarchive identityを付与する。
pub fn create_archive(
    notes: Vec<Note>,
    note_acl: Vec<marginalis_domain::ArchivedNoteAclEntry>,
) -> Archive {
    Archive {
        format: ARCHIVE_FORMAT.into(),
        adocweave_package_version: PINNED_ADOCWEAVE_PACKAGE_VERSION.into(),
        note_profile_version: NOTE_PROFILE_VERSION,
        notes,
        note_acl,
    }
}

/// archiveのidentityと全ノートが現行のAsciiDoc profileに一致することを検証する。
///
/// ID、時刻、format markerの構造検証は永続化adapterが担う。本関数はparserを必要とする
/// content policyだけを入力境界で検証し、SQLite adapterをAsciiDoc実装から独立させる。
pub fn validate_archive(archive: &Archive) -> Result<(), ArchiveValidationError> {
    if archive.format != ARCHIVE_FORMAT
        || archive.adocweave_package_version != PINNED_ADOCWEAVE_PACKAGE_VERSION
        || archive.note_profile_version != NOTE_PROFILE_VERSION
    {
        return Err(ArchiveValidationError);
    }
    for note in &archive.notes {
        let normalized = validate_note_draft(NoteDraft {
            title: note.title().to_owned(),
            body: note.body().to_owned(),
            tags: note.tags().to_vec(),
        })
        .map_err(|_| ArchiveValidationError)?;
        if normalized.title != note.title()
            || normalized.body != note.body()
            || normalized.tags != note.tags()
        {
            return Err(ArchiveValidationError);
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ArchiveValidationError;

impl fmt::Display for ArchiveValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("archive contains a note outside the current AsciiDoc profile")
    }
}

impl std::error::Error for ArchiveValidationError {}

fn format_unix_millis(value: UnixMillis) -> Result<String, ExportError> {
    let nanos = i128::from(value.get()) * 1_000_000;
    OffsetDateTime::from_unix_timestamp_nanos(nanos)
        .map_err(|_| ExportError::InvalidTimestamp)?
        .format(&Rfc3339)
        .map_err(|_| ExportError::InvalidTimestamp)
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use marginalis_domain::{Archive, EntityId, Identity, NoteId, Revision};

    use super::*;

    fn note(body: &str) -> Note {
        Note::restore(
            NoteId::new(
                EntityId::from_str("0197c9bc-0000-7000-8000-000000000001").expect("UUIDv7"),
            ),
            Identity::new("https://id.example.test".into(), "alice".into()).expect("valid owner"),
            "A title".into(),
            body.into(),
            vec!["Research".into()],
            UnixMillis::new(0),
            UnixMillis::new(1_000),
            Revision::INITIAL,
            None,
        )
        .expect("consistent note")
    }

    #[test]
    fn package_version_matches_the_pinned_specification() {
        assert_eq!(ADOCWEAVE_SOURCE_REVISION.len(), 40);
        verify_runtime_package_version().expect("pinned version");
    }

    #[test]
    fn export_contains_server_managed_metadata() {
        let exported = export_note(&note("body")).expect("export");
        assert!(exported.contains(":note-id: 0197c9bc-0000-7000-8000-000000000001"));
        assert!(exported.contains(":creator-subject: alice"));
        assert!(exported.ends_with("\n\nbody"));
    }

    #[test]
    fn note_deserialization_rejects_identity_attribute_injection() {
        let mut serialized = serde_json::to_value(note("body")).expect("serialize note");
        serialized["creator_subject"] = "alice\n:admin: true".into();
        assert!(serde_json::from_value::<Note>(serialized).is_err());
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
    fn every_profile_example_is_accepted_by_the_validator() {
        for example in note_profile().examples {
            validate_note_draft(NoteDraft {
                title: "Example".into(),
                body: example.body.into(),
                tags: Vec::new(),
            })
            .unwrap_or_else(|errors| panic!("{} must be valid: {errors:?}", example.kind));
        }
    }

    #[test]
    fn strict_syntax_is_rejected_before_storage_and_rendering() {
        let body = "[role=test]";
        let errors = validate_note_draft(NoteDraft {
            title: "Title".into(),
            body: body.into(),
            tags: Vec::new(),
        })
        .expect_err("unsupported strict syntax");
        assert!(
            errors
                .iter()
                .any(|error| error.code == NoteValidationCode::AsciiDocParseFailed)
        );
        assert_eq!(render_note_html(&note(body)), Err(RenderError));
    }

    #[test]
    fn raw_tag_count_uses_the_advertised_limit() {
        let errors = validate_note_draft(NoteDraft {
            title: "Title".into(),
            body: "Body.".into(),
            tags: vec!["duplicate".into(); MAX_TAGS + 1],
        })
        .expect_err("raw tag count");
        assert!(
            errors
                .iter()
                .any(|error| error.code == NoteValidationCode::TooManyTags)
        );
    }

    #[test]
    fn draft_validation_rejects_oversized_body_before_parsing() {
        let errors = validate_note_draft(NoteDraft {
            title: "Title".into(),
            body: "x".repeat(MAX_NOTE_BODY_BYTES + 1),
            tags: Vec::new(),
        })
        .expect_err("oversized body");
        assert!(
            errors
                .iter()
                .any(|error| error.code == NoteValidationCode::BodyTooLarge)
        );
    }

    #[test]
    fn diagnostics_identify_fields_and_utf8_body_ranges() {
        let body = "日本\n\n[source,brainfuck]\n----\n+\n----";
        let errors = validate_note_draft(NoteDraft {
            title: String::new(),
            body: body.into(),
            tags: vec!["valid".into(), "bad,tag".into()],
        })
        .expect_err("invalid draft");
        assert_eq!(errors[0].target, NoteValidationTarget::Title);
        assert_eq!(errors[0].span, None);
        assert_eq!(errors[1].target, NoteValidationTarget::Tag { index: 1 });
        assert_eq!(errors[1].span, None);

        let source = errors
            .iter()
            .find(|error| error.code == NoteValidationCode::UnsupportedSourceLanguage)
            .expect("source diagnostic");
        let expected_start = u32::try_from(body.find("brainfuck").expect("language")).unwrap();
        assert_eq!(source.target, NoteValidationTarget::Body);
        assert_eq!(
            source.span,
            Some(Utf8ByteSpan {
                start: expected_start,
                end: expected_start + 9,
            })
        );
    }

    #[test]
    fn archive_validation_rejects_non_normalized_notes() {
        let base = note("safe body");
        let archived_note = Note::restore(
            base.note_id(),
            base.owner().clone(),
            base.title().to_owned(),
            base.body().to_owned(),
            vec![" duplicate ".into(), "duplicate".into()],
            base.created_at(),
            base.updated_at(),
            base.revision(),
            base.deleted_at(),
        )
        .expect("structurally consistent note");
        let archive = create_archive(vec![archived_note], Vec::new());
        assert_eq!(validate_archive(&archive), Err(ArchiveValidationError));
    }

    #[test]
    fn archive_validation_requires_exact_contract_identity() {
        let archive = create_archive(Vec::new(), Vec::new());
        assert_eq!(archive.format, ARCHIVE_FORMAT);
        assert_eq!(
            archive.adocweave_package_version,
            PINNED_ADOCWEAVE_PACKAGE_VERSION
        );
        assert_eq!(archive.note_profile_version, NOTE_PROFILE_VERSION);
        assert_eq!(validate_archive(&archive), Ok(()));

        for invalid in [
            Archive {
                format: "marginalis-archive-1".into(),
                ..archive.clone()
            },
            Archive {
                adocweave_package_version: "0.6.1".into(),
                ..archive.clone()
            },
            Archive {
                note_profile_version: NOTE_PROFILE_VERSION + 1,
                ..archive
            },
        ] {
            assert_eq!(validate_archive(&invalid), Err(ArchiveValidationError));
        }
    }

    #[test]
    fn unsafe_content_is_rejected_before_rendering() {
        for body in [
            "include::secret[]",
            "pass:[<script>alert(1)</script>]",
            "link:javascript:alert(1)[unsafe]",
            "image::https://example.test/a.png[]",
            "xref:../other.adoc[relative]",
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
    fn authored_forbidden_rules_have_reachable_validation_cases() {
        let cases = [
            (
                "include::secret[]",
                NoteValidationCode::IncludeDirectiveDisabled,
            ),
            (
                "pass:[<script>alert(1)</script>]",
                NoteValidationCode::InlinePassthroughDisabled,
            ),
            (
                "[pass]\n++++\n<script>alert(1)</script>\n++++",
                NoteValidationCode::BlockPassthroughDisabled,
            ),
            (
                "[[same]]\nOne.\n\n[[same]]\nTwo.",
                NoteValidationCode::DuplicateAnchor,
            ),
            (
                "xref:other.adoc[other]",
                NoteValidationCode::ExternalReferenceDisabled,
            ),
            (
                "link:javascript:alert(1)[unsafe]",
                NoteValidationCode::InvalidUrlScheme,
            ),
            (
                "image::https://example.test/a.png[]",
                NoteValidationCode::ResourceDisabled,
            ),
            (
                "[source,brainfuck]\n----\n+\n----",
                NoteValidationCode::UnsupportedSourceLanguage,
            ),
        ];
        for (body, expected) in cases {
            let errors = validate_note_draft(NoteDraft {
                title: "Title".into(),
                body: body.into(),
                tags: Vec::new(),
            })
            .expect_err("forbidden rule");
            assert!(
                errors.iter().any(|error| error.code == expected),
                "{expected:?} must be reachable from {body:?}: {errors:?}"
            );
        }
    }

    #[test]
    fn safe_content_renders_without_raw_markup() {
        let html = render_note_html(&note("[[local]]\nA *safe* paragraph. See <<local>>."))
            .expect("render");
        assert!(html.contains("<strong>safe</strong>"));
        assert!(html.contains("href=\"#local\""));
    }

    #[test]
    fn v010_and_v011_fixed_inputs_preserve_note_profile_semantics() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../fixtures/v0.10.1-note-profile.json"))
                .expect("v0.10.1 fixture");
        assert_eq!(fixture["adocweave_package_version"], "0.10.1");
        for case in fixture["cases"].as_array().expect("cases") {
            let name = case["name"].as_str().expect("case name");
            let body = case["body"].as_str().expect("case body");
            let expected_diagnostics = case["diagnostics"].as_array().expect("diagnostics");
            match validate_note_draft(NoteDraft {
                title: "Title".into(),
                body: body.into(),
                tags: Vec::new(),
            }) {
                Ok(_) => {
                    assert_eq!(case["accepted"], true, "{name}");
                    assert!(expected_diagnostics.is_empty(), "{name}");
                    let html = render_note_html(&note(body)).expect("render accepted fixture");
                    assert_eq!(
                        html,
                        case["html"].as_str().expect("complete HTML"),
                        "{name}"
                    );
                }
                Err(errors) => {
                    assert_eq!(case["accepted"], false, "{name}");
                    let actual = errors
                        .iter()
                        .map(|error| {
                            let target = match error.target {
                                NoteValidationTarget::Title => serde_json::json!("title"),
                                NoteValidationTarget::Body => serde_json::json!("body"),
                                NoteValidationTarget::Tag { index } => {
                                    serde_json::json!({ "tag": index })
                                }
                                NoteValidationTarget::Tags => serde_json::json!("tags"),
                                NoteValidationTarget::AclEntry { .. } => {
                                    unreachable!("AsciiDoc validation does not inspect ACL entries")
                                }
                            };
                            let span = error.span.expect("fixture body diagnostic span");
                            serde_json::json!({
                                "code": error.code.as_str(),
                                "target": target,
                                "start": span.start,
                                "end": span.end,
                                "message": error.message,
                            })
                        })
                        .collect::<Vec<_>>();
                    assert_eq!(
                        serde_json::Value::Array(actual),
                        serde_json::Value::Array(expected_diagnostics.clone()),
                        "{name}: {errors:?}"
                    );
                    assert_eq!(render_note_html(&note(body)), Err(RenderError), "{name}");
                }
            }
        }

        let explicit_noheader = render_note_html(&note(
            "[%noheader]\n|===\n|Name |Value\n\n|alpha |one\n|===",
        ))
        .expect("render table without header");
        assert!(!explicit_noheader.contains("<thead>"));
    }

    #[test]
    fn v011_configuration_keeps_url_and_lint_responsibilities_explicit() {
        use adocweave::output::diagnostics::{
            ASCIIDOC_FILE_LINK, MACRO_BOUNDARY, NON_ASCIIDOC_XREF,
        };

        let analysis = note_analysis_options();
        assert!(!analysis.diagnostics.lint.authored_url_policy.allow_relative);
        assert!(analysis.diagnostics.lint.rule(ASCIIDOC_FILE_LINK).enabled);
        assert!(analysis.diagnostics.lint.rule(NON_ASCIIDOC_XREF).enabled);
        assert!(!analysis.diagnostics.lint.rule(MACRO_BOUNDARY).enabled);

        let rendering = note_render_policy();
        assert!(!rendering.active_urls.allow_authored_relative);
        assert!(!rendering.active_urls.allow_resolved_relative);
        assert!(rendering.active_urls.allow_resolved_root_relative);
        assert!(!rendering.active_urls.allow_data_uris);
        assert_eq!(
            rendering.active_urls.allowed_schemes,
            ["http".to_owned(), "https".to_owned()].into()
        );
        assert_eq!(
            rendering.source_languages.allowed,
            Some(
                DEFAULT_SOURCE_LANGUAGES
                    .iter()
                    .map(|language| (*language).to_owned())
                    .collect()
            )
        );
        assert_eq!(
            rendering.source_languages.unknown,
            adocweave::output::html::UnknownSourceLanguage::Diagnostic
        );
        assert_eq!(
            rendering.math_languages.allowed,
            [adocweave::semantic::MathLanguage::Latex].into()
        );
        assert!(!rendering.resources.images);
        assert!(!rendering.resources.media);
        assert_eq!(note_output_limits().max_output_bytes, 50 * 1024 * 1024);
        assert!(html_is_within_output_limits(
            "1234",
            &OutputLimits {
                max_output_bytes: 4
            }
        ));
        assert!(!html_is_within_output_limits(
            "12345",
            &OutputLimits {
                max_output_bytes: 4
            }
        ));
    }

    #[test]
    fn v011_rejects_malformed_authored_urls() {
        let body = "link:https://example.test/%ZZ[bad percent encoding]";
        let errors = validate_note_draft(NoteDraft {
            title: "Title".into(),
            body: body.into(),
            tags: Vec::new(),
        })
        .expect_err("malformed URL");
        assert!(
            errors
                .iter()
                .any(|error| error.code == NoteValidationCode::InvalidUrlScheme),
            "{errors:?}"
        );

        assert!(
            !note_analysis_options()
                .diagnostics
                .lint
                .authored_url_policy
                .allows("//example.test/network-path")
        );
    }

    #[test]
    fn table_cell_source_blocks_follow_the_language_profile() {
        let body = "|===\na|\n[source,rust]\n----\nfn main() {}\n----\n|===";
        assert!(
            validate_note_draft(NoteDraft {
                title: "Title".into(),
                body: body.into(),
                tags: Vec::new(),
            })
            .is_ok()
        );

        let unsupported = body.replace("source,rust", "source,brainfuck");
        let errors = validate_note_draft(NoteDraft {
            title: "Title".into(),
            body: unsupported,
            tags: Vec::new(),
        })
        .expect_err("unsupported source language");
        assert!(
            errors
                .iter()
                .any(|error| error.code == NoteValidationCode::UnsupportedSourceLanguage)
        );
    }

    #[test]
    fn note_scheme_accepts_only_uuid_v7_targets() {
        let target = "0197c9bc-0000-7000-8000-000000000002";
        let body = format!("xref:note:{target}#evidence[根拠]");
        assert!(
            validate_note_draft(NoteDraft {
                title: "Title".into(),
                body: body.clone(),
                tags: Vec::new(),
            })
            .is_ok()
        );
        let queries = note_reference_queries(&body).expect("reference queries");
        assert_eq!(queries.len(), 1);
        assert_eq!(queries[0].target_note_id.to_string(), target);
        assert_eq!(queries[0].anchor.as_deref(), Some("evidence"));

        for forbidden in [
            "xref:note:not-a-uuid[invalid]",
            "xref:note:550e8400-e29b-41d4-a716-446655440000[v4]",
            "xref:https:example.test[unknown scheme]",
            "xref:other.adoc[document]",
        ] {
            let errors = validate_note_draft(NoteDraft {
                title: "Title".into(),
                body: forbidden.into(),
                tags: Vec::new(),
            })
            .expect_err("external reference");
            assert!(
                errors
                    .iter()
                    .any(|error| error.code == NoteValidationCode::ExternalReferenceDisabled),
                "{forbidden}: {errors:?}"
            );
        }
    }

    #[test]
    fn resolved_note_references_fill_titles_and_hide_missing_targets() {
        let target = "0197c9bc-0000-7000-8000-000000000002";
        let source = note(&format!(
            "xref:note:{target}[] xref:note:{target}[指定ラベル]"
        ));
        let html = render_note_html_with_references(
            &source,
            &[
                NoteReferenceResolution::Visible {
                    reference_index: 0,
                    href: format!("/marginalis/notes/{target}"),
                    title: "参照先タイトル".into(),
                    missing_anchor: false,
                },
                NoteReferenceResolution::Visible {
                    reference_index: 1,
                    href: format!("/marginalis/notes/{target}"),
                    title: "参照先タイトル".into(),
                    missing_anchor: false,
                },
            ],
        )
        .expect("resolved HTML");
        assert!(html.contains(&format!("href=\"/marginalis/notes/{target}\"")));
        assert!(html.contains(">参照先タイトル</a>"));
        assert!(html.contains(">指定ラベル</a>"));

        let hidden = render_note_html_with_references(
            &source,
            &[
                NoteReferenceResolution::Hidden { reference_index: 0 },
                NoteReferenceResolution::Hidden { reference_index: 1 },
            ],
        )
        .expect("hidden HTML");
        assert!(!hidden.contains(target));
        assert!(!hidden.contains("参照先タイトル"));
        assert!(hidden.contains("指定ラベル"));
        assert!(!hidden.contains("href="));
    }
}
