use std::collections::BTreeMap;

use adocweave::output::diagnostics::Severity;
use adocweave::resolution::ReferenceKey;
use marginalis_application::{
    NoteAdvisorySeverity, NoteReferenceQuery, NoteValidationCode, NoteValidationDiagnostic,
    NoteValidationTarget, Utf8ByteSpan, ValidatedNoteDraft,
};
use marginalis_domain::{EntityId, NoteDraft, NoteId};
use unicode_normalization::UnicodeNormalization;

use crate::configuration::analysis_options;
use crate::policy::{
    advisory_diagnostic, diagnostic, diagnostic_sort_key, span, validate_note_content_profile,
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
        || analysis.reference_queries().into_iter().any(|query| {
            matches!(
                query.target,
                ReferenceKey::Scheme {
                    ref scheme,
                    ref locator,
                    ..
                } if scheme == "note" && locator.parse::<EntityId>().is_err()
            )
        })
    {
        return Err(RenderError);
    }
    Ok(analysis)
}

pub(crate) fn reference_queries(source: &str) -> Result<Vec<NoteReferenceQuery>, RenderError> {
    let result = reference_queries_from_analysis(&analyze_valid_source(source)?);
    if result.invalid_spans.is_empty() {
        Ok(result.queries)
    } else {
        Err(RenderError)
    }
}

#[derive(Debug, Default)]
struct ReferenceQueryAnalysis {
    queries: Vec<NoteReferenceQuery>,
    invalid_spans: Vec<Utf8ByteSpan>,
}

fn reference_queries_from_analysis(analysis: &adocweave::Analysis) -> ReferenceQueryAnalysis {
    let mut result = ReferenceQueryAnalysis::default();
    for (reference_index, query) in analysis.reference_queries().into_iter().enumerate() {
        if let ReferenceKey::Scheme {
            scheme,
            locator,
            anchor,
        } = query.target
        {
            if scheme != "note" {
                continue;
            }
            match locator.parse::<EntityId>() {
                Ok(id) => result.queries.push(NoteReferenceQuery {
                    reference_index,
                    target_note_id: NoteId::new(id),
                    anchor,
                }),
                Err(_) => result.invalid_spans.push(span(query.source_range)),
            }
        }
    }
    result
}

pub(crate) fn has_anchor(source: &str, anchor: &str) -> Result<bool, RenderError> {
    Ok(analyze_valid_source(source)?
        .reference_targets()
        .iter()
        .any(|target| target.id == anchor))
}

pub(crate) fn validate_draft(
    draft: NoteDraft,
) -> Result<ValidatedNoteDraft, Vec<NoteValidationDiagnostic>> {
    let mut errors = Vec::new();
    let mut advisories = Vec::new();
    let mut reference_queries = Vec::new();
    let mut title = String::new();
    let mut tags = BTreeMap::new();
    if draft.source.len() > MAX_NOTE_SOURCE_BYTES {
        errors.push(diagnostic(
            NoteValidationCode::SourceTooLarge,
            NoteValidationTarget::Source,
            None,
        ));
    } else {
        match adocweave::Engine::new(analysis_options()).analyze(&draft.source) {
            Ok(analysis) => {
                let reference_analysis = reference_queries_from_analysis(&analysis);
                reference_queries = reference_analysis.queries;
                errors.extend(
                    reference_analysis
                        .invalid_spans
                        .into_iter()
                        .map(|source_span| {
                            diagnostic(
                                NoteValidationCode::InvalidNoteReference,
                                NoteValidationTarget::Source,
                                Some(source_span),
                            )
                        }),
                );
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
                    .attribute_environment()
                    .final_values()
                    .get("tags")
                    .map(String::as_str)
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
                        .map(|item| {
                            diagnostic(
                                NoteValidationCode::AsciiDocParseFailed,
                                NoteValidationTarget::Source,
                                Some(span(item.range)),
                            )
                        }),
                );
                advisories.extend(analysis.diagnostics().iter().filter_map(|item| {
                    public_advisory_severity(item.severity).map(|severity| {
                        advisory_diagnostic(
                            item.code.as_str(),
                            &item.message,
                            severity,
                            NoteValidationTarget::Source,
                            Some(span(item.range)),
                        )
                    })
                }));
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
    errors.sort_by_key(|item| diagnostic_sort_key(&item.target, item.span, &item.code));
    advisories.sort_by_key(|item| diagnostic_sort_key(&item.target, item.span, &item.code));
    if errors.is_empty() {
        Ok(ValidatedNoteDraft {
            draft: NoteDraft {
                source: draft.source,
                title,
                tags: tags.into_values().collect(),
            },
            diagnostics: advisories,
            reference_queries,
        })
    } else {
        Err(errors)
    }
}

const fn public_advisory_severity(severity: Severity) -> Option<NoteAdvisorySeverity> {
    match severity {
        Severity::Error => None,
        Severity::Warning => Some(NoteAdvisorySeverity::Warning),
        Severity::Information => Some(NoteAdvisorySeverity::Information),
        Severity::Hint => Some(NoteAdvisorySeverity::Hint),
    }
}

#[cfg(test)]
mod tests {
    use marginalis_application::{
        NoteAdvisorySeverity, NoteValidationCode, NoteValidationTarget, Utf8ByteSpan,
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
        assert_eq!(warning.severity, NoteAdvisorySeverity::Warning);
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
    fn multiline_list_item_preserves_a_warning_range_on_its_continuation_line() {
        let source = concat!(
            "= 調査結果\n\n",
            "* 最初の行\n",
            "  本文xref:note:0197c9bc-0000-7000-8000-000000000002[先行調査]\n",
        );
        let validated = validate_draft(NoteDraft {
            source: source.into(),
            title: String::new(),
            tags: Vec::new(),
        })
        .expect("warning does not reject a multiline list item");
        let warning = validated
            .diagnostics
            .iter()
            .find(|item| item.code == "macro-boundary")
            .expect("macro boundary warning");
        let span = warning.span.expect("source range");

        assert_eq!(&source[span.start as usize..span.end as usize], "xref");
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
    fn every_invalid_note_locator_has_one_dedicated_diagnostic() {
        let source = "= Test\n\nxref:note:not-a-note[first]\n\nxref:note:also-invalid[second]";
        let errors = validate_draft(NoteDraft {
            source: source.into(),
            title: String::new(),
            tags: Vec::new(),
        })
        .expect_err("invalid note locators");

        let invalid_references = errors
            .iter()
            .filter(|error| error.code == NoteValidationCode::InvalidNoteReference.as_str())
            .collect::<Vec<_>>();
        assert_eq!(invalid_references.len(), 2);
        assert!(
            errors
                .iter()
                .all(|error| error.code != NoteValidationCode::ExternalReferenceDisabled.as_str())
        );
        assert!(invalid_references.iter().all(|error| error.span.is_some()));
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
