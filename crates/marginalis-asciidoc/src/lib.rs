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
use marginalis_application::{LogicalSnapshot, NoteAclSnapshotEntry};
use marginalis_application::{
    NoteContent, NoteContentError, NoteProfile, NoteValidationCode, NoteValidationDiagnostic,
    NoteValidationTarget,
};
use marginalis_domain::{
    EntityId, Identity, Note, NoteDraft, NoteId, NotePermission, Revision, UnixMillis,
};
use serde::{Deserialize, Serialize};
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
pub const NOTE_PROFILE_VERSION: u32 = 3;
pub const MAX_TITLE_CHARACTERS: usize = 200;
pub const MAX_NOTE_SOURCE_BYTES: usize = 512 * 1024;
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
        source: &str,
    ) -> Result<Vec<marginalis_application::NoteReferenceQuery>, NoteContentError> {
        note_reference_queries(source)
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

    fn has_anchor(&self, source: &str, anchor: &str) -> Result<bool, NoteContentError> {
        note_has_anchor(source, anchor).map_err(|_| NoteContentError)
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

/// 正本のAsciiDoc文書を書き出せない理由。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExportError;

impl fmt::Display for ExportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("canonical note source cannot be exported")
    }
}

impl std::error::Error for ExportError {}

/// 利用者が保存した完全なAsciiDoc文書をそのままexportする。
pub fn export_note(note: &Note) -> Result<String, ExportError> {
    Ok(note.source().to_owned())
}

/// SQLite正本を再検証した上で、固定RenderPolicyの安全なHTMLへ変換する。
pub fn render_note_html(note: &Note) -> Result<String, RenderError> {
    render_note_html_with_references(note, &[])
}

/// ノート参照を抽出し、ホスト側でACL判定するための問い合わせを返す。
pub fn note_reference_queries(source: &str) -> Result<Vec<NoteReferenceQuery>, RenderError> {
    let analysis = adocweave::Engine::new(note_analysis_options())
        .analyze(source)
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
pub fn note_has_anchor(source: &str, anchor: &str) -> Result<bool, RenderError> {
    let analysis = adocweave::Engine::new(note_analysis_options())
        .analyze(source)
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

/// 完全なAsciiDoc文書を検証し、題名とタグの検索用投影を導出する。
pub fn validate_note_draft(draft: NoteDraft) -> Result<NoteDraft, Vec<NoteValidationDiagnostic>> {
    let mut errors = Vec::new();
    let mut title = String::new();
    let mut tags = BTreeMap::new();
    if draft.source.len() > MAX_NOTE_SOURCE_BYTES {
        errors.push(diagnostic(
            NoteValidationCode::SourceTooLarge,
            NoteValidationTarget::Source,
            None,
        ));
    } else {
        match adocweave::Engine::new(note_analysis_options()).analyze(&draft.source) {
            Ok(analysis) => {
                title = analysis
                    .reference_targets()
                    .iter()
                    .find(|target| {
                        target.kind == adocweave::semantic::ReferenceTargetKind::DocumentTitle
                    })
                    .map(|target| target.label.trim().nfc().collect::<String>())
                    .unwrap_or_default();
                if title.is_empty() || title.chars().count() > MAX_TITLE_CHARACTERS {
                    errors.push(diagnostic(
                        NoteValidationCode::InvalidTitle,
                        NoteValidationTarget::Source,
                        None,
                    ));
                }
                let header_end = analysis.document().header().end;
                for occurrence in analysis.document_attribute_occurrences() {
                    const ALLOWED: &[&str] = &[
                        "tags",
                        "sectnums",
                        "toc",
                        "toclevels",
                        "stem",
                        "source-language",
                    ];
                    if occurrence.range.end() > header_end
                        || !ALLOWED.contains(&occurrence.name.as_str())
                    {
                        errors.push(diagnostic(
                            NoteValidationCode::UnsupportedDocumentAttribute,
                            NoteValidationTarget::Source,
                            Some(span(occurrence.name_range)),
                        ));
                    }
                }
                let raw_tags = analysis
                    .presentation()
                    .attributes()
                    .get("tags")
                    .unwrap_or_default();
                let tag_values = raw_tags.split(',').collect::<Vec<_>>();
                if tag_values.len() > MAX_TAGS {
                    errors.push(diagnostic(
                        NoteValidationCode::TooManyTags,
                        NoteValidationTarget::Source,
                        None,
                    ));
                }
                for tag in tag_values {
                    let display = tag.trim().nfc().collect::<String>();
                    if display.is_empty() {
                        continue;
                    }
                    if display.contains(['\n', '\r'])
                        || display.chars().count() > MAX_TAG_CHARACTERS
                    {
                        errors.push(diagnostic(
                            NoteValidationCode::InvalidTag,
                            NoteValidationTarget::Source,
                            None,
                        ));
                    } else {
                        tags.entry(display.to_lowercase()).or_insert(display);
                    }
                }
                errors.extend(
                    analysis
                        .diagnostics()
                        .iter()
                        .filter(|diagnostic| diagnostic.severity == Severity::Error)
                        .map(|adoc_diagnostic| {
                            diagnostic(
                                NoteValidationCode::AsciiDocParseFailed,
                                NoteValidationTarget::Source,
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
                                NoteValidationTarget::Source,
                                Some(span(error.range)),
                            )
                        }),
                );
            }
            Err(_) => errors.push(diagnostic(
                NoteValidationCode::AsciiDocParseFailed,
                NoteValidationTarget::Source,
                None,
            )),
        }
    }
    if errors.is_empty() {
        Ok(NoteDraft {
            source: draft.source,
            title,
            tags: tags.into_values().collect(),
        })
    } else {
        errors.sort_by_key(diagnostic_sort_key);
        Err(errors)
    }
}

pub const ARCHIVE_FORMAT: &str = "marginalis-archive-7";

/// JSON archiveの転送形式。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Archive {
    pub format: String,
    pub adocweave_package_version: String,
    pub note_profile_version: u32,
    pub notes: Vec<ArchiveNote>,
    pub note_acl: Vec<ArchiveAclEntry>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ArchiveNote {
    pub note_id: String,
    pub creator_issuer: String,
    pub creator_subject: String,
    pub source: String,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub revision: i64,
    pub deleted_at_ms: Option<i64>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ArchiveAclEntry {
    pub note_id: String,
    pub issuer: String,
    pub subject: String,
    pub permission: ArchivePermission,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ArchivePermission {
    Read,
    Edit,
}

/// 論理スナップショットへ現行のarchive識別情報を付与する。
pub fn create_archive(snapshot: &LogicalSnapshot) -> Archive {
    Archive {
        format: ARCHIVE_FORMAT.into(),
        adocweave_package_version: PINNED_ADOCWEAVE_PACKAGE_VERSION.into(),
        note_profile_version: NOTE_PROFILE_VERSION,
        notes: snapshot
            .notes()
            .iter()
            .map(|note| ArchiveNote {
                note_id: note.note_id().to_string(),
                creator_issuer: note.creator_issuer().to_owned(),
                creator_subject: note.creator_subject().to_owned(),
                source: note.source().to_owned(),
                created_at_ms: note.created_at().get(),
                updated_at_ms: note.updated_at().get(),
                revision: note.revision().get(),
                deleted_at_ms: note.deleted_at().map(UnixMillis::get),
            })
            .collect(),
        note_acl: snapshot
            .note_acl()
            .iter()
            .map(|entry| ArchiveAclEntry {
                note_id: entry.note_id().to_string(),
                issuer: entry.identity().issuer().to_owned(),
                subject: entry.identity().subject().to_owned(),
                permission: match entry.permission() {
                    NotePermission::Read => ArchivePermission::Read,
                    NotePermission::Edit => ArchivePermission::Edit,
                },
            })
            .collect(),
    }
}

/// JSON archiveを検証し、保存方式に依存しない論理スナップショットへ変換する。
pub fn validate_archive(archive: &Archive) -> Result<LogicalSnapshot, ArchiveValidationError> {
    if archive.format != ARCHIVE_FORMAT
        || archive.adocweave_package_version != PINNED_ADOCWEAVE_PACKAGE_VERSION
        || archive.note_profile_version != NOTE_PROFILE_VERSION
    {
        return Err(ArchiveValidationError);
    }
    let notes = archive
        .notes
        .iter()
        .map(|note| {
            let normalized = validate_note_draft(NoteDraft {
                source: note.source.clone(),
                title: String::new(),
                tags: Vec::new(),
            })
            .map_err(|_| ArchiveValidationError)?;
            let note_id = note
                .note_id
                .parse::<EntityId>()
                .map(NoteId::new)
                .map_err(|_| ArchiveValidationError)?;
            let creator = Identity::new(note.creator_issuer.clone(), note.creator_subject.clone())
                .map_err(|_| ArchiveValidationError)?;
            let revision = Revision::new(note.revision).map_err(|_| ArchiveValidationError)?;
            Note::restore(
                note_id,
                creator,
                normalized.title,
                note.source.clone(),
                normalized.tags,
                UnixMillis::new(note.created_at_ms),
                UnixMillis::new(note.updated_at_ms),
                revision,
                note.deleted_at_ms.map(UnixMillis::new),
            )
            .map_err(|_| ArchiveValidationError)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let note_acl = archive
        .note_acl
        .iter()
        .map(|entry| {
            let note_id = entry
                .note_id
                .parse::<EntityId>()
                .map(NoteId::new)
                .map_err(|_| ArchiveValidationError)?;
            Ok(NoteAclSnapshotEntry::new(
                note_id,
                Identity::new(entry.issuer.clone(), entry.subject.clone())
                    .map_err(|_| ArchiveValidationError)?,
                match entry.permission {
                    ArchivePermission::Read => NotePermission::Read,
                    ArchivePermission::Edit => NotePermission::Edit,
                },
            ))
        })
        .collect::<Result<Vec<_>, ArchiveValidationError>>()?;
    LogicalSnapshot::new(notes, note_acl).map_err(|_| ArchiveValidationError)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ArchiveValidationError;

impl fmt::Display for ArchiveValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("archive is inconsistent with the current archive contract")
    }
}

impl std::error::Error for ArchiveValidationError {}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use marginalis_domain::{EntityId, Identity, NoteId, Revision};

    use super::*;

    fn note(body: &str) -> Note {
        let source = format!("= A title\n:tags: Research\n\n{body}");
        Note::restore(
            NoteId::new(
                EntityId::from_str("0197c9bc-0000-7000-8000-000000000001").expect("UUIDv7"),
            ),
            Identity::new("https://id.example.test".into(), "alice".into()).expect("valid owner"),
            "A title".into(),
            source,
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
    fn export_preserves_the_authored_document_without_server_metadata() {
        let exported = export_note(&note("body")).expect("export");
        assert!(!exported.contains(":note-id:"));
        assert!(!exported.contains(":creator-subject:"));
        assert!(exported.starts_with("= A title\n:tags: Research"));
        assert!(exported.ends_with("\n\nbody"));
    }

    #[test]
    fn draft_validation_normalizes_tags() {
        let draft = validate_note_draft(NoteDraft {
            title: String::new(),
            source: "= Title\n:tags: Rust, rust\n\nsafe body".into(),
            tags: Vec::new(),
        })
        .expect("valid draft");
        assert_eq!(draft.title, "Title");
        assert_eq!(draft.tags, vec!["Rust"]);
    }

    #[test]
    fn complete_document_derives_metadata_and_enables_section_numbers() {
        let source = "= 新規ノート\n:tags: new, research\n:sectnums:\n\n== 見出し1\n\nこれはテスト用の本文です。";
        let draft = validate_note_draft(NoteDraft {
            source: source.into(),
            title: String::new(),
            tags: Vec::new(),
        })
        .expect("valid complete document");
        assert_eq!(draft.title, "新規ノート");
        assert_eq!(draft.tags, ["new", "research"]);

        let note = Note::create(
            NoteId::new(
                EntityId::from_str("0197c9bc-0000-7000-8000-000000000001").expect("UUIDv7"),
            ),
            &Identity::new("https://id.example.test".into(), "alice".into()).expect("valid owner"),
            draft,
            UnixMillis::new(0),
        );
        let html = render_note_html(&note).expect("render");
        assert!(html.contains(">1. 見出し1</h1>"));
    }

    #[test]
    fn server_managed_attributes_are_rejected_from_authored_source() {
        let errors = validate_note_draft(NoteDraft {
            source: "= Test\n:note-id: forged\n\nBody.".into(),
            title: String::new(),
            tags: Vec::new(),
        })
        .expect_err("server-managed attribute");
        assert!(errors.iter().any(|error| {
            error.code == NoteValidationCode::UnsupportedDocumentAttribute
                && error.target == NoteValidationTarget::Source
                && error.span.is_some()
        }));
    }

    #[test]
    fn every_profile_example_is_accepted_by_the_validator() {
        for example in note_profile().examples {
            validate_note_draft(NoteDraft {
                title: "Example".into(),
                source: format!("= Example\n\n{}", example.body),
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
            source: format!("= Test\n\n{body}"),
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
            title: String::new(),
            source: format!(
                "= Test\n:tags: {}\n\nBody.",
                vec!["duplicate"; MAX_TAGS + 1].join(",")
            ),
            tags: Vec::new(),
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
            source: "x".repeat(MAX_NOTE_SOURCE_BYTES + 1),
            tags: Vec::new(),
        })
        .expect_err("oversized body");
        assert!(
            errors
                .iter()
                .any(|error| error.code == NoteValidationCode::SourceTooLarge)
        );
    }

    #[test]
    fn diagnostics_identify_fields_and_utf8_body_ranges() {
        let body = "日本\n\n[source,brainfuck]\n----\n+\n----";
        let errors = validate_note_draft(NoteDraft {
            title: String::new(),
            source: format!("= Test\n\n{body}"),
            tags: Vec::new(),
        })
        .expect_err("invalid draft");

        let source = errors
            .iter()
            .find(|error| error.code == NoteValidationCode::UnsupportedSourceLanguage)
            .expect("source diagnostic");
        let complete = format!("= Test\n\n{body}");
        let expected_start = u32::try_from(complete.find("brainfuck").expect("language")).unwrap();
        assert_eq!(source.target, NoteValidationTarget::Source);
        assert_eq!(
            source.span,
            Some(Utf8ByteSpan {
                start: expected_start,
                end: expected_start + 9,
            })
        );
    }

    #[test]
    fn archive_validation_rejects_invalid_authored_source() {
        let snapshot = LogicalSnapshot::new(vec![note("safe body")], Vec::new()).expect("snapshot");
        let mut archive = create_archive(&snapshot);
        archive.notes[0].source = "本文だけ".into();
        assert_eq!(validate_archive(&archive), Err(ArchiveValidationError));
    }

    #[test]
    fn archive_validation_requires_exact_contract_identity() {
        let snapshot = LogicalSnapshot::new(Vec::new(), Vec::new()).expect("empty snapshot");
        let archive = create_archive(&snapshot);
        assert_eq!(archive.format, ARCHIVE_FORMAT);
        assert_eq!(
            archive.adocweave_package_version,
            PINNED_ADOCWEAVE_PACKAGE_VERSION
        );
        assert_eq!(archive.note_profile_version, NOTE_PROFILE_VERSION);
        assert_eq!(validate_archive(&archive), Ok(snapshot));

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
    fn archive_acl_round_trip_preserves_the_complete_identity() {
        let note = note("safe body");
        let reader =
            Identity::new(note.creator_issuer().into(), "reader".into()).expect("reader identity");
        let snapshot = LogicalSnapshot::new(
            vec![note.clone()],
            vec![NoteAclSnapshotEntry::new(
                note.note_id(),
                reader,
                NotePermission::Read,
            )],
        )
        .expect("snapshot");
        let archive = create_archive(&snapshot);
        assert_eq!(archive.note_acl[0].issuer, note.creator_issuer());
        assert_eq!(validate_archive(&archive), Ok(snapshot));
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
                    source: format!("= Test\n\n{body}"),
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
                source: format!("= Test\n\n{body}"),
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
                source: format!("= Test\n\n{body}"),
                tags: Vec::new(),
            }) {
                Ok(_) => {
                    assert_eq!(case["accepted"], true, "{name}");
                    assert!(expected_diagnostics.is_empty(), "{name}");
                    render_note_html(&note(body)).expect("render accepted fixture");
                }
                Err(errors) => {
                    assert_eq!(case["accepted"], false, "{name}");
                    let actual = errors
                        .iter()
                        .map(|error| {
                            let target = match error.target {
                                NoteValidationTarget::Source => serde_json::json!("source"),
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
                    let actual_codes = actual
                        .iter()
                        .map(|diagnostic| diagnostic["code"].clone())
                        .collect::<Vec<_>>();
                    let expected_codes = expected_diagnostics
                        .iter()
                        .map(|diagnostic| diagnostic["code"].clone())
                        .collect::<Vec<_>>();
                    assert_eq!(actual_codes, expected_codes, "{name}: {errors:?}");
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
    fn complete_display_fixture_renders_supported_blocks_together() {
        let source = "= VMで作成したノート\n:tags: 受入試験, 日本語\n:stem: latexmath\n:sectnums:\n:toc:\n\n== 表示要素\n\n* 箇条書き\n* 二つ目\n\n[quote]\n____\n引用した文章\n____\n\n|===\n|項目 |値\n\n|日本語 |絵文字😀\n|===\n\n[source,rust]\n----\nfn main() {}\n----\n\nstem:[x^2 + y^2]\n\n[latexmath]\n++++\nx^2 + y^2\n++++\n\n日本語と絵文字😀\r\n\n*強調した本文*";
        let draft = validate_note_draft(NoteDraft {
            title: String::new(),
            source: source.into(),
            tags: Vec::new(),
        })
        .expect("complete display fixture");
        let note = Note::restore(
            NoteId::new(
                EntityId::from_str("0197c9bc-0000-7000-8000-000000000001").expect("UUIDv7"),
            ),
            Identity::new("https://id.example.test".into(), "alice".into()).expect("valid owner"),
            draft.title,
            draft.source,
            draft.tags,
            UnixMillis::new(0),
            UnixMillis::new(1_000),
            Revision::INITIAL,
            None,
        )
        .expect("consistent note");
        let html = render_note_html(&note).expect("render complete display fixture");
        for expected in [
            "class=\"toc\"",
            "<blockquote>",
            "<table",
            "language-rust",
            "math-latex",
        ] {
            assert!(html.contains(expected), "missing {expected}: {html}");
        }
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
            source: format!("= Test\n\n{body}"),
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
                source: format!("= Test\n\n{body}"),
                tags: Vec::new(),
            })
            .is_ok()
        );

        let unsupported = body.replace("source,rust", "source,brainfuck");
        let errors = validate_note_draft(NoteDraft {
            title: "Title".into(),
            source: format!("= Test\n\n{unsupported}"),
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
                source: format!("= Test\n\n{body}"),
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
                source: format!("= Test\n\n{forbidden}"),
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
