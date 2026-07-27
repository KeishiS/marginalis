//! SQLite正本のAsciiDoc検証、可搬化、安全なHTML描画を担うadapter。

use core::fmt;
use std::collections::BTreeMap;

use adocweave::SyntaxMode;
use adocweave::output::diagnostics::Severity;
use adocweave::output::html::{RenderPolicy, render};
#[cfg(test)]
use marginalis_application::Utf8ByteSpan;
use marginalis_application::{NoteValidationCode, NoteValidationDiagnostic, NoteValidationTarget};
use marginalis_domain::{ARCHIVE_FORMAT, Archive, Note, NoteBundle, NoteDraft, UnixMillis};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use unicode_normalization::UnicodeNormalization;

mod policy;

#[cfg(test)]
use policy::FORBIDDEN_RULES;
pub use policy::note_profile;
use policy::{diagnostic, diagnostic_sort_key, span, validate_note_content_profile};

pub const ADOCWEAVE_SOURCE_REVISION: &str = "3cd213fed631a6855859e71b74ee772134ce5834";

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
pub const PINNED_ADOCWEAVE_PACKAGE_VERSION: &str = "0.10.1";
/// Marginalisが保存時に適用するノート入力規則の版。
pub const NOTE_PROFILE_VERSION: u32 = 1;
pub const MAX_TITLE_CHARACTERS: usize = 200;
pub const MAX_NOTE_BODY_BYTES: usize = 512 * 1024;
pub const MAX_TAGS: usize = 50;
pub const MAX_TAG_CHARACTERS: usize = 64;

fn note_parse_options() -> adocweave::ParseOptions {
    adocweave::ParseOptions {
        syntax_mode: SyntaxMode::Strict,
        ..Default::default()
    }
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
pub struct ExportError;

impl fmt::Display for ExportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("canonical note timestamp is outside the RFC 3339 range")
    }
}

impl std::error::Error for ExportError {}

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
    let analysis = adocweave::Engine::new(note_parse_options())
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
    let output = render(analysis.document(), &RenderPolicy::default());
    if output
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == Severity::Error)
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
        match adocweave::Engine::new(note_parse_options()).analyze(&draft.body) {
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
pub fn create_archive(notes: Vec<NoteBundle>) -> Archive {
    Archive {
        format: ARCHIVE_FORMAT.into(),
        adocweave_package_version: PINNED_ADOCWEAVE_PACKAGE_VERSION.into(),
        note_profile_version: NOTE_PROFILE_VERSION,
        notes,
    }
}

/// archiveのidentityと全ノートが現行のAsciiDoc profileに一致することを検証する。
///
/// ID、時刻、ACL、format markerの構造検証は永続化adapterが担う。本関数はparserを必要とする
/// content policyだけを入力境界で検証し、SQLite adapterをAsciiDoc実装から独立させる。
pub fn validate_archive(archive: &Archive) -> Result<(), ArchiveValidationError> {
    if archive.format != ARCHIVE_FORMAT
        || archive.adocweave_package_version != PINNED_ADOCWEAVE_PACKAGE_VERSION
        || archive.note_profile_version != NOTE_PROFILE_VERSION
    {
        return Err(ArchiveValidationError);
    }
    for bundle in &archive.notes {
        let normalized = validate_note_draft(NoteDraft {
            title: bundle.note.title.clone(),
            body: bundle.note.body.clone(),
            tags: bundle.note.tags.clone(),
        })
        .map_err(|_| ArchiveValidationError)?;
        if normalized.title != bundle.note.title
            || normalized.body != bundle.note.body
            || normalized.tags != bundle.note.tags
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
        .map_err(|_| ExportError)?
        .format(&Rfc3339)
        .map_err(|_| ExportError)
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use marginalis_domain::{Archive, EntityId, NoteBundle, NoteId};

    use super::*;

    fn note(body: &str) -> Note {
        Note {
            note_id: NoteId::new(
                EntityId::from_str("0197c9bc-0000-7000-8000-000000000001").expect("UUIDv7"),
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
        let mut archived_note = note("safe body");
        archived_note.tags = vec![" duplicate ".into(), "duplicate".into()];
        let archive = create_archive(vec![NoteBundle {
            note: archived_note,
            acl: Vec::new(),
        }]);
        assert_eq!(validate_archive(&archive), Err(ArchiveValidationError));
    }

    #[test]
    fn archive_validation_requires_exact_contract_identity() {
        let archive = create_archive(Vec::new());
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
            "xref:note:0197c9bc-0000-7000-8000-000000000001[note]",
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
    fn every_forbidden_rule_has_a_reachable_validation_case() {
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
        assert_eq!(cases.len(), FORBIDDEN_RULES.len());
    }

    #[test]
    fn safe_content_renders_without_raw_markup() {
        let html = render_note_html(&note("[[local]]\nA *safe* paragraph. See <<local>>."))
            .expect("render");
        assert!(html.contains("<strong>safe</strong>"));
        assert!(html.contains("href=\"#local\""));
    }

    #[test]
    fn v010_parsing_semantics_are_part_of_the_note_profile() {
        let monospace = render_note_html(&note("snake_`code`\n\n日本``和文``日本 😀``emoji``😀"))
            .expect("render monospace");
        assert!(monospace.contains("snake_`code`"));
        assert!(monospace.contains("日本<code>和文</code>日本"));
        assert!(monospace.contains("😀<code>emoji</code>😀"));

        let implicit_header = render_note_html(&note("|===\n|Name |Value\n\n|alpha |one\n|==="))
            .expect("render table");
        assert!(implicit_header.contains("<thead>"));
        assert!(implicit_header.contains("<th class="));

        let explicit_noheader = render_note_html(&note(
            "[%noheader]\n|===\n|Name |Value\n\n|alpha |one\n|===",
        ))
        .expect("render table without header");
        assert!(!explicit_noheader.contains("<thead>"));
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
}
