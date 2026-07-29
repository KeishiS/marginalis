use std::collections::BTreeMap;

use adocweave::output::diagnostics::Severity;
use adocweave::resolution::ReferenceKey;
use marginalis_application::{
    NoteDiagnostic, NoteDiagnosticSeverity, NoteReferenceQuery, NoteValidationCode,
    NoteValidationTarget, Utf8ByteSpan, ValidatedNoteDraft,
};
use marginalis_domain::{EntityId, NoteDraft, NoteId};
use unicode_normalization::UnicodeNormalization;

use crate::configuration::analysis_options;
use crate::policy::{
    diagnostic, diagnostic_sort_key, span, validate_note_content_profile, warning_diagnostic,
};
use crate::{
    MAX_NOTE_SOURCE_BYTES, MAX_TAG_CHARACTERS, MAX_TAGS, MAX_TITLE_CHARACTERS, RenderError,
};

pub(crate) fn analyze_valid_source(source: &str) -> Result<adocweave::Analysis, RenderError> {
    let analysis = adocweave::Engine::new(analysis_options())
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
    Ok(analysis)
}

pub(crate) fn reference_queries(source: &str) -> Result<Vec<NoteReferenceQuery>, RenderError> {
    reference_queries_from_analysis(&analyze_valid_source(source)?).map_err(|_| RenderError)
}

fn reference_queries_from_analysis(
    analysis: &adocweave::Analysis,
) -> Result<Vec<NoteReferenceQuery>, Utf8ByteSpan> {
    analysis
        .reference_queries()
        .into_iter()
        .enumerate()
        .filter_map(|(reference_index, query)| {
            let source_span = span(query.source_range);
            match query.target {
                ReferenceKey::Scheme {
                    scheme,
                    locator,
                    anchor,
                } if scheme == "note" => Some(
                    locator
                        .parse::<EntityId>()
                        .map(|id| NoteReferenceQuery {
                            reference_index,
                            target_note_id: NoteId::new(id),
                            anchor,
                        })
                        .map_err(|_| source_span),
                ),
                _ => None,
            }
        })
        .collect()
}

pub(crate) fn has_anchor(source: &str, anchor: &str) -> Result<bool, RenderError> {
    Ok(analyze_valid_source(source)?
        .reference_targets()
        .iter()
        .any(|target| target.id == anchor))
}

pub(crate) fn validate_draft(draft: NoteDraft) -> Result<ValidatedNoteDraft, Vec<NoteDiagnostic>> {
    let mut diagnostics = Vec::new();
    let mut reference_queries = Vec::new();
    let mut title = String::new();
    let mut tags = BTreeMap::new();
    if draft.source.len() > MAX_NOTE_SOURCE_BYTES {
        diagnostics.push(diagnostic(
            NoteValidationCode::SourceTooLarge,
            NoteValidationTarget::Source,
            None,
        ));
    } else {
        match adocweave::Engine::new(analysis_options()).analyze(&draft.source) {
            Ok(analysis) => {
                match reference_queries_from_analysis(&analysis) {
                    Ok(queries) => reference_queries = queries,
                    Err(source_span) => diagnostics.push(diagnostic(
                        NoteValidationCode::InvalidNoteReference,
                        NoteValidationTarget::Source,
                        Some(source_span),
                    )),
                }
                title = analysis
                    .reference_targets()
                    .iter()
                    .find(|target| {
                        target.kind == adocweave::semantic::ReferenceTargetKind::DocumentTitle
                    })
                    .map(|target| target.label.trim().nfc().collect::<String>())
                    .unwrap_or_default();
                if title.is_empty() || title.chars().count() > MAX_TITLE_CHARACTERS {
                    diagnostics.push(diagnostic(
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
                        diagnostics.push(diagnostic(
                            NoteValidationCode::UnsupportedDocumentAttribute,
                            NoteValidationTarget::Source,
                            Some(span(occurrence.name_range)),
                        ));
                    }
                }
                let raw_tags = analysis
                    .attribute_environment()
                    .final_values()
                    .get("tags")
                    .map(String::as_str)
                    .unwrap_or_default();
                let tag_values = raw_tags.split(',').collect::<Vec<_>>();
                if tag_values.len() > MAX_TAGS {
                    diagnostics.push(diagnostic(
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
                        diagnostics.push(diagnostic(
                            NoteValidationCode::InvalidTag,
                            NoteValidationTarget::Source,
                            None,
                        ));
                    } else {
                        tags.entry(display.to_lowercase()).or_insert(display);
                    }
                }
                diagnostics.extend(
                    analysis
                        .diagnostics()
                        .iter()
                        .filter(|diagnostic| diagnostic.severity == Severity::Error)
                        .map(|item| {
                            diagnostic(
                                NoteValidationCode::AsciiDocParseFailed,
                                NoteValidationTarget::Source,
                                Some(span(item.range)),
                            )
                        }),
                );
                diagnostics.extend(analysis.diagnostics().iter().filter_map(|item| {
                    public_advisory_severity(item.severity).map(|severity| {
                        warning_diagnostic(
                            item.code.as_str(),
                            &item.message,
                            severity,
                            NoteValidationTarget::Source,
                            Some(span(item.range)),
                        )
                    })
                }));
                diagnostics.extend(validate_note_content_profile(&analysis).into_iter().map(
                    |error| {
                        diagnostic(
                            error.code,
                            NoteValidationTarget::Source,
                            Some(span(error.range)),
                        )
                    },
                ));
            }
            Err(_) => diagnostics.push(diagnostic(
                NoteValidationCode::AsciiDocParseFailed,
                NoteValidationTarget::Source,
                None,
            )),
        }
    }
    diagnostics.sort_by_key(diagnostic_sort_key);
    if diagnostics
        .iter()
        .any(|item| item.severity == NoteDiagnosticSeverity::Error)
    {
        Err(diagnostics)
    } else {
        Ok(ValidatedNoteDraft {
            draft: NoteDraft {
                source: draft.source,
                title,
                tags: tags.into_values().collect(),
            },
            diagnostics,
            reference_queries,
        })
    }
}

const fn public_advisory_severity(severity: Severity) -> Option<NoteDiagnosticSeverity> {
    match severity {
        Severity::Error => None,
        Severity::Warning => Some(NoteDiagnosticSeverity::Warning),
        Severity::Information => Some(NoteDiagnosticSeverity::Information),
        Severity::Hint => Some(NoteDiagnosticSeverity::Hint),
    }
}

#[cfg(test)]
mod tests {
    use marginalis_application::{
        NoteDiagnosticSeverity, NoteValidationCode, NoteValidationTarget, Utf8ByteSpan,
    };
    use marginalis_domain::NoteDraft;

    use super::*;

    #[test]
    fn complete_document_derives_normalized_metadata() {
        let draft = validate_draft(NoteDraft {
            source: "= 新規ノート\n:tags: Rust, rust\n:sectnums:\n\n== 見出し".into(),
            title: String::new(),
            tags: Vec::new(),
        })
        .expect("valid document");
        assert_eq!(draft.draft.title, "新規ノート");
        assert_eq!(draft.draft.tags, ["Rust"]);
        assert!(draft.diagnostics.is_empty());
    }

    #[test]
    fn macro_boundary_is_a_non_blocking_warning_with_a_utf8_range() {
        let source = concat!(
            "= 調査結果\n\n",
            "この結果はxref:note:0197c9bc-0000-7000-8000-000000000002[先行調査]",
            "に記載されています。",
        );
        let validated = validate_draft(NoteDraft {
            source: source.into(),
            title: String::new(),
            tags: Vec::new(),
        })
        .expect("warning does not reject a draft");
        let warning = validated
            .diagnostics
            .iter()
            .find(|item| item.code == "macro-boundary")
            .expect("macro boundary warning");
        assert_eq!(warning.severity, NoteDiagnosticSeverity::Warning);
        let span = warning.span.expect("source range");
        assert_eq!(&source[span.start as usize..span.end as usize], "xref");

        let corrected = source.replace("はxref:", "は xref:");
        let validated = validate_draft(NoteDraft {
            source: corrected,
            title: String::new(),
            tags: Vec::new(),
        })
        .expect("corrected draft");
        assert!(
            validated
                .diagnostics
                .iter()
                .all(|item| item.code != "macro-boundary")
        );
    }

    #[test]
    fn forbidden_content_returns_a_stable_diagnostic() {
        let errors = validate_draft(NoteDraft {
            source: "= Test\n\ninclude::secret[]".into(),
            title: String::new(),
            tags: Vec::new(),
        })
        .expect_err("include is disabled");
        assert!(
            errors.iter().any(|error| {
                error.code == NoteValidationCode::IncludeDirectiveDisabled.as_str()
            })
        );
    }

    #[test]
    fn note_reference_uses_the_application_contract() {
        let target = "0197c9bc-0000-7000-8000-000000000002";
        let queries =
            reference_queries(&format!("xref:note:{target}#evidence[根拠]")).expect("queries");
        assert_eq!(queries[0].target_note_id.to_string(), target);
        assert_eq!(queries[0].anchor.as_deref(), Some("evidence"));
    }

    #[test]
    fn diagnostics_preserve_utf8_byte_ranges() {
        let body = "日本\n\n[source,brainfuck]\n----\n+\n----";
        let source = format!("= Test\n\n{body}");
        let errors = validate_draft(NoteDraft {
            source: source.clone(),
            title: String::new(),
            tags: Vec::new(),
        })
        .expect_err("unsupported source language");
        let diagnostic = errors
            .iter()
            .find(|error| error.code == NoteValidationCode::UnsupportedSourceLanguage.as_str())
            .expect("language diagnostic");
        let start = u32::try_from(source.find("brainfuck").expect("language")).expect("span");
        assert_eq!(diagnostic.target, NoteValidationTarget::Source);
        assert_eq!(
            diagnostic.span,
            Some(Utf8ByteSpan {
                start,
                end: start + 9,
            })
        );
    }

    #[test]
    fn tags_follow_the_final_source_ordered_attribute_environment() {
        let redefine = validated("= Note\n:tags: research\n:tags: rust\n\nbody");
        assert_eq!(redefine.tags, ["rust"]);

        let unset = validated("= Note\n:tags: research\n:tags!:\n\nbody");
        assert!(unset.tags.is_empty());

        let reference =
            validated("= Note\n:source-language: rust\n:tags: {source-language}\n\nbody");
        assert_eq!(reference.tags, ["rust"]);
    }

    #[test]
    fn multiline_tags_use_folded_values_and_reject_authored_line_breaks() {
        let soft = validated(concat!("= Note\n:tags: research, \\", "\n  rust\n\nbody"));
        assert_eq!(soft.tags, ["research", "rust"]);

        let errors = validate_draft(NoteDraft {
            source: concat!("= Note\n:tags: research, + \\", "\n  rust\n\nbody").into(),
            title: String::new(),
            tags: Vec::new(),
        })
        .expect_err("hard continuation introduces a forbidden line break");
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, NoteValidationCode::InvalidTag.as_str());
    }

    #[test]
    fn attribute_operations_after_the_header_are_rejected() {
        let errors = validate_draft(NoteDraft {
            source: "= Note\n\n:tags: body\nbody".into(),
            title: String::new(),
            tags: Vec::new(),
        })
        .expect_err("body attribute operation");
        assert_eq!(errors.len(), 1);
        assert_eq!(
            errors[0].code,
            NoteValidationCode::UnsupportedDocumentAttribute.as_str()
        );
        assert!(errors[0].span.is_some());
    }

    fn validated(source: &str) -> NoteDraft {
        validate_draft(NoteDraft {
            source: source.into(),
            title: String::new(),
            tags: Vec::new(),
        })
        .expect("valid draft")
        .draft
    }
}
