use std::collections::BTreeMap;

use adocweave::output::diagnostics::Severity;
use adocweave::resolution::ReferenceKey;
use marginalis_application::{
    NoteReferenceQuery, NoteValidationCode, NoteValidationDiagnostic, NoteValidationTarget,
};
use marginalis_domain::{EntityId, NoteDraft, NoteId};
use unicode_normalization::UnicodeNormalization;

use crate::configuration::analysis_options;
use crate::policy::{diagnostic, diagnostic_sort_key, span, validate_note_content_profile};
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
    analyze_valid_source(source)?
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
                    .parse::<EntityId>()
                    .map(|id| NoteReferenceQuery {
                        reference_index,
                        target_note_id: NoteId::new(id),
                        anchor,
                    })
                    .map_err(|_| RenderError),
            ),
            _ => None,
        })
        .collect()
}

pub(crate) fn has_anchor(source: &str, anchor: &str) -> Result<bool, RenderError> {
    Ok(analyze_valid_source(source)?
        .reference_targets()
        .iter()
        .any(|target| target.id == anchor))
}

pub(crate) fn validate_draft(draft: NoteDraft) -> Result<NoteDraft, Vec<NoteValidationDiagnostic>> {
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
        match adocweave::Engine::new(analysis_options()).analyze(&draft.source) {
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
                        .map(|item| {
                            diagnostic(
                                NoteValidationCode::AsciiDocParseFailed,
                                NoteValidationTarget::Source,
                                Some(span(item.range)),
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

#[cfg(test)]
mod tests {
    use marginalis_application::{NoteValidationCode, NoteValidationTarget, Utf8ByteSpan};
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
        assert_eq!(draft.title, "新規ノート");
        assert_eq!(draft.tags, ["Rust"]);
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
            errors
                .iter()
                .any(|error| { error.code == NoteValidationCode::IncludeDirectiveDisabled })
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
            .find(|error| error.code == NoteValidationCode::UnsupportedSourceLanguage)
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
}
