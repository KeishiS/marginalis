use std::collections::BTreeMap;

use adocweave::output::diagnostics::Severity;
use adocweave::resolution::ReferenceKey;
use marginalis_application::{
    CitationStyle, NoteAdvisorySeverity, NoteCitationQuery, NoteReferenceQuery, NoteValidationCode,
    NoteValidationDiagnostic, ValidatedNoteDraft,
};
use marginalis_domain::{
    CITATION_STYLE_DOCUMENT_ATTRIBUTE, EntityId, NOTE_POLICY, NoteDraft, NoteId,
    NoteValidationTarget, TAGS_DOCUMENT_ATTRIBUTE, Utf8ByteSpan,
};
use unicode_normalization::UnicodeNormalization;

use crate::RenderError;
use crate::configuration::{UNPROCESSED_DIRECTIVE_RULE, analysis_options};
use crate::policy::{
    advisory_diagnostic, diagnostic, diagnostic_sort_key, source_position, span,
    validate_note_content_profile,
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

pub(crate) fn citation_queries(source: &str) -> Result<Vec<NoteCitationQuery>, RenderError> {
    Ok(citation_queries_from_analysis(&analyze_valid_source(
        source,
    )?))
}

pub(crate) fn citation_style(source: &str) -> Result<CitationStyle, RenderError> {
    Ok(citation_style_from_analysis(&analyze_valid_source(source)?))
}

/// 文書headerが選んだ引用の表示規則を読み取る。
///
/// 属性を書かないノートは既定の規則になる。許可しない値は入力検査が拒否するため、ここへ
/// 到達する値は正本の一覧にあるものだけである。
fn citation_style_from_analysis(analysis: &adocweave::Analysis) -> CitationStyle {
    analysis
        .attribute_environment()
        .final_values()
        .get(CITATION_STYLE_DOCUMENT_ATTRIBUTE)
        .map(|value| CitationStyle::from_attribute(value.trim()))
        .unwrap_or_default()
}

/// `cite:`が名指すcitation keyを、文献ライブラリへの問い合わせへ直す。
///
/// AdocWeaveは位置引数をcitation key、名前付き引数を引用の補足として返す。Marginalisは
/// `locator`だけを引用へ添える値として扱い、他の名前付き引数は表示に使わない。
fn citation_queries_from_analysis(analysis: &adocweave::Analysis) -> Vec<NoteCitationQuery> {
    analysis
        .citations()
        .into_iter()
        .enumerate()
        .map(|(citation_index, citation)| {
            let source_span = span(citation.range);
            NoteCitationQuery {
                citation_index,
                keys: citation.keys.into_iter().map(|key| key.value).collect(),
                locator: citation
                    .attributes
                    .into_iter()
                    .find(|attribute| attribute.name.as_deref() == Some("locator"))
                    .map(|attribute| attribute.value),
                span: source_span,
                position: source_position(analysis.source_document(), source_span)
                    .expect("AdocWeave citation ranges are valid source positions"),
            }
        })
        .collect()
}

/// 本文を省いた文書の構成を返す。
///
/// 見出しはAdocWeaveの構造投影から読み、行番号は診断と同じ位置変換を使う。文書題名は
/// ノートのtitleが別に持つため、一覧から除く。
pub(crate) fn outline(source: &str) -> Result<marginalis_application::NoteOutline, RenderError> {
    use adocweave::semantic::SectionKind;

    let analysis = analyze_valid_source(source)?;
    let document = analysis.source_document();
    let line_count = source_line_count(source);
    let mut sections: Vec<marginalis_application::NoteOutlineSection> = Vec::new();
    for heading in analysis.structure().headings() {
        if heading.kind == SectionKind::DocumentTitle {
            continue;
        }
        let start_line = source_position(document, span(heading.range))
            .ok_or(RenderError)?
            .line as usize;
        // 明示した`[#id]`はアンカー側の範囲がIDの出所になり、自動生成のIDは
        // 見出し本文の範囲と一致する。一致する場合はIDを推測して返さない。
        let anchor = (heading.id_range != heading.title_range).then(|| heading.id.clone());
        sections.push(marginalis_application::NoteOutlineSection {
            level: heading.level,
            title: heading.title.clone(),
            anchor,
            start_line,
            end_line: line_count,
        });
    }
    // 節の末尾は、次に現れる同じ深さ以浅の見出しの直前の行とする(子節を含む階層範囲)。
    let boundaries: Vec<(usize, u8)> = sections
        .iter()
        .map(|section| (section.start_line, section.level))
        .collect();
    for (index, section) in sections.iter_mut().enumerate() {
        if let Some((next_start, _)) = boundaries[index + 1..]
            .iter()
            .find(|(_, level)| *level <= section.level)
        {
            section.end_line = next_start - 1;
        }
    }
    Ok(marginalis_application::NoteOutline {
        sections,
        line_count,
    })
}

/// 原文の総行数。末尾改行は行として数えない。
fn source_line_count(source: &str) -> usize {
    if source.is_empty() {
        0
    } else if source.ends_with('\n') {
        source.split('\n').count() - 1
    } else {
        source.split('\n').count()
    }
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
    let mut citation_queries = Vec::new();
    let mut title = String::new();
    let mut tags = BTreeMap::new();
    let mut citation_style = CitationStyle::default();
    if draft.source.len() > NOTE_POLICY.max_source_bytes {
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
                citation_queries = citation_queries_from_analysis(&analysis);
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
                if title.is_empty() || title.chars().count() > NOTE_POLICY.max_title_characters {
                    errors.push(diagnostic(
                        NoteValidationCode::InvalidTitle,
                        NoteValidationTarget::Source,
                        None,
                    ));
                }
                let header_end = analysis.document().header().end;
                for occurrence in analysis.document_attribute_occurrences() {
                    // 許可する名前の正本は`NOTE_POLICY`にある。`get_note_profile`が広告する
                    // 一覧も同じ値から導くため、受理する入力と公開する制約が食い違わない。
                    if occurrence.range.end() > header_end
                        || !NOTE_POLICY
                            .allowed_document_attributes
                            .contains(&occurrence.name.as_str())
                    {
                        errors.push(diagnostic(
                            NoteValidationCode::UnsupportedDocumentAttribute,
                            NoteValidationTarget::Source,
                            Some(span(occurrence.name_range)),
                        ));
                    }
                }
                // 許可しない値の引用スタイルは、属性名と同じ位置で拒否する。任意のCSL
                // スタイル名を受け取らないため、サーバー上でCSLを実行することがない。
                if let Some(value) = analysis
                    .attribute_environment()
                    .final_values()
                    .get(CITATION_STYLE_DOCUMENT_ATTRIBUTE)
                    && !NOTE_POLICY.allowed_citation_styles.contains(&value.trim())
                {
                    let name_range = analysis
                        .document_attribute_occurrences()
                        .iter()
                        .filter(|occurrence| occurrence.name == CITATION_STYLE_DOCUMENT_ATTRIBUTE)
                        .map(|occurrence| span(occurrence.name_range))
                        .next_back();
                    errors.push(diagnostic(
                        NoteValidationCode::UnsupportedCitationStyle,
                        NoteValidationTarget::Source,
                        name_range,
                    ));
                }
                citation_style = citation_style_from_analysis(&analysis);
                let raw_tags = analysis
                    .attribute_environment()
                    .final_values()
                    .get(TAGS_DOCUMENT_ATTRIBUTE)
                    .map(String::as_str)
                    .unwrap_or_default();
                let tag_values = raw_tags.split(',').collect::<Vec<_>>();
                if tag_values.len() > NOTE_POLICY.max_tags {
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
                        || display.chars().count() > NOTE_POLICY.max_tag_characters
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
                            // 条件分岐と取り込みのdirectiveは構文の誤りではない。理由が
                            // 「AsciiDocとして読めない」になると、書いた人は直し方が分からない。
                            let code = if item.code.as_str() == UNPROCESSED_DIRECTIVE_RULE {
                                NoteValidationCode::PreprocessorDirectiveDisabled
                            } else {
                                NoteValidationCode::AsciiDocParseFailed
                            };
                            diagnostic(code, NoteValidationTarget::Source, Some(span(item.range)))
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
    if let Ok(document) = adocweave::text::SourceDocument::new(&draft.source) {
        for item in &mut errors {
            if item.target == NoteValidationTarget::Source {
                item.position = source_position(
                    &document,
                    item.span.unwrap_or(Utf8ByteSpan { start: 0, end: 0 }),
                );
            }
        }
        for item in &mut advisories {
            if item.target == NoteValidationTarget::Source {
                item.position = source_position(
                    &document,
                    item.span.unwrap_or(Utf8ByteSpan { start: 0, end: 0 }),
                );
            }
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
            citation_queries,
            citation_style,
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
    use marginalis_application::{NoteAdvisorySeverity, NoteValidationCode};
    use marginalis_domain::{
        DOCUMENT_ATTRIBUTE_PREFIX, NoteDraft, NoteValidationTarget, Utf8ByteSpan,
    };

    use super::*;

    #[test]
    fn complete_document_derives_normalized_metadata() {
        let draft = validate_draft(NoteDraft {
            source: "= 新規ノート\n:marginalis-tags: Rust, rust\n:sectnums:\n\n== 見出し".into(),
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
    fn monospace_boundary_is_a_non_blocking_warning() {
        let source = "= 調査結果\n\n本文は`code`です。";
        let validated = validate_draft(NoteDraft {
            source: source.into(),
            title: String::new(),
            tags: Vec::new(),
        })
        .expect("warning does not reject a draft");
        let warning = validated
            .diagnostics
            .iter()
            .find(|item| item.code == "monospace-boundary")
            .expect("monospace boundary warning");
        assert_eq!(warning.severity, NoteAdvisorySeverity::Warning);

        let corrected = source.replace("`code`", "``code``");
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
                .all(|item| item.code != "monospace-boundary")
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
    fn diagnostics_include_lsp_compatible_positions_for_crlf_and_non_bmp_text() {
        let source = "= Test\r\n\r\n😀 日本語  \r\n";
        let validated = validate_draft(NoteDraft {
            source: source.into(),
            title: String::new(),
            tags: Vec::new(),
        })
        .expect("trailing whitespace is an advisory");
        let diagnostic = validated
            .diagnostics
            .iter()
            .find(|item| item.code == "trailing-whitespace")
            .expect("trailing whitespace diagnostic");
        assert_eq!(
            diagnostic.position,
            Some(marginalis_application::NoteSourcePosition { line: 3, column: 7 })
        );
        let span = diagnostic.span.expect("source span");
        assert_eq!(&source[span.start as usize..span.end as usize], "  ");
    }

    #[test]
    fn source_diagnostic_without_a_span_points_to_the_document_start() {
        let errors = validate_draft(NoteDraft {
            source: "本文だけです。".into(),
            title: String::new(),
            tags: Vec::new(),
        })
        .expect_err("document title is required");
        let diagnostic = errors
            .iter()
            .find(|item| item.code == NoteValidationCode::InvalidTitle.as_str())
            .expect("title diagnostic");

        assert_eq!(diagnostic.span, None);
        assert_eq!(
            diagnostic.position,
            Some(marginalis_application::NoteSourcePosition { line: 1, column: 1 })
        );
    }

    #[test]
    fn tags_follow_the_final_source_ordered_attribute_environment() {
        let redefine =
            validated("= Note\n:marginalis-tags: research\n:marginalis-tags: rust\n\nbody");
        assert_eq!(redefine.tags, ["rust"]);

        let unset = validated("= Note\n:marginalis-tags: research\n:marginalis-tags!:\n\nbody");
        assert!(unset.tags.is_empty());

        let reference = validated(
            "= Note\n:source-language: rust\n:marginalis-tags: {source-language}\n\nbody",
        );
        assert_eq!(reference.tags, ["rust"]);
    }

    #[test]
    fn multiline_tags_use_folded_values_and_reject_authored_line_breaks() {
        let soft = validated(concat!(
            "= Note\n:marginalis-tags: research, \\",
            "\n  rust\n\nbody"
        ));
        assert_eq!(soft.tags, ["research", "rust"]);

        let errors = validate_draft(NoteDraft {
            source: concat!(
                "= Note\n:marginalis-tags: research, + \\",
                "\n  rust\n\nbody"
            )
            .into(),
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
            source: "= Note\n\n:marginalis-tags: body\nbody".into(),
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

    /// 入力検査と`get_note_profile`の広告が、同じ一覧から導かれることを確かめる。
    ///
    /// 以前は許可リストがこのmoduleの中の定数にあり、公開する側は何も知らなかった。MCP
    /// クライアントは「対応していない属性は拒否される」とだけ知らされ、何が使えるかを
    /// 知る手段がなかった。
    #[test]
    fn the_advertised_document_attributes_are_exactly_the_accepted_ones() {
        let advertised = crate::policy::note_profile()
            .syntax
            .allowed_document_attributes;
        assert_eq!(advertised, NOTE_POLICY.allowed_document_attributes.to_vec());

        // 広告した属性はすべて受理する。
        for name in &advertised {
            let source = match *name {
                // 値を要求する属性は、値を添えないと別の理由で落ちる。
                "toclevels" => "= Note\n:toclevels: 2\n\n== 見出し\n\n本文".to_owned(),
                "stem" => "= Note\n:stem: latexmath\n\n本文".to_owned(),
                "source-language" => "= Note\n:source-language: rust\n\n本文".to_owned(),
                TAGS_DOCUMENT_ATTRIBUTE => {
                    format!("= Note\n:{TAGS_DOCUMENT_ATTRIBUTE}: 研究\n\n本文")
                }
                CITATION_STYLE_DOCUMENT_ATTRIBUTE => format!(
                    "= Note\n:{CITATION_STYLE_DOCUMENT_ATTRIBUTE}: {}\n\n本文",
                    NOTE_POLICY.allowed_citation_styles[0]
                ),
                other => format!("= Note\n:{other}:\n\n== 見出し\n\n本文"),
            };
            assert!(
                validate_draft(NoteDraft {
                    source,
                    title: String::new(),
                    tags: Vec::new(),
                })
                .is_ok(),
                "広告した属性を受理できません: {name}"
            );
        }

        // 一覧に無い属性は拒む。広告と検査が同じ一覧を見ている証拠になる。
        assert!(!NOTE_POLICY.allowed_document_attributes.contains(&"author"));
        let errors = validate_draft(NoteDraft {
            source: "= Note\n:author: Someone\n\n本文".into(),
            title: String::new(),
            tags: Vec::new(),
        })
        .expect_err("一覧に無い属性");
        assert!(
            errors.iter().any(
                |error| error.code == NoteValidationCode::UnsupportedDocumentAttribute.as_str()
            )
        );
    }

    /// Marginalis独自の属性は接頭辞で始め、AsciiDocの組込み属性と名前で区別できるようにする。
    ///
    /// 接頭辞が無いと、本文を読んだだけでは他のAsciiDoc処理系で意味を持つ属性かどうかが
    /// 分からない。組込み属性は仕様が定めた名前をそのまま使うため、接頭辞を付けない。
    #[test]
    fn marginalis_specific_attributes_carry_the_prefix() {
        let builtin = ["sectnums", "toc", "toclevels", "stem", "source-language"];
        for name in NOTE_POLICY.allowed_document_attributes {
            assert_eq!(
                name.starts_with(DOCUMENT_ATTRIBUTE_PREFIX),
                !builtin.contains(name),
                "独自属性と組込み属性の区別が接頭辞と合いません: {name}"
            );
        }
    }

    /// 引用スタイルの広告と入力検査が、同じ正本から導かれている。
    ///
    /// 正本から値を1つ外すと、広告と検査の両方が同時に変わる。片方だけを直すと、選べると
    /// 広告した値が保存できない、あるいはその逆が起きる。
    #[test]
    fn the_advertised_citation_styles_are_exactly_the_accepted_ones() {
        let advertised = crate::policy::note_profile().syntax.allowed_citation_styles;
        assert_eq!(advertised, NOTE_POLICY.allowed_citation_styles.to_vec());

        for style in &advertised {
            let source = format!("= Note\n:{CITATION_STYLE_DOCUMENT_ATTRIBUTE}: {style}\n\n本文");
            assert!(
                validate_draft(NoteDraft {
                    source,
                    title: String::new(),
                    tags: Vec::new(),
                })
                .is_ok(),
                "広告したスタイルを受理できません: {style}"
            );
        }
    }

    /// 一覧に無い値は保存前に拒否し、属性名の位置を示す。
    ///
    /// 任意のCSLスタイル名を受け取らないため、サーバー上でCSLを実行することがない。
    #[test]
    fn a_citation_style_outside_the_list_is_rejected_with_its_position() {
        assert!(!NOTE_POLICY.allowed_citation_styles.contains(&"apa"));
        let errors = validate_draft(NoteDraft {
            source: format!("= Note\n:{CITATION_STYLE_DOCUMENT_ATTRIBUTE}: apa\n\n本文"),
            title: String::new(),
            tags: Vec::new(),
        })
        .expect_err("一覧に無いスタイル");
        let rejected = errors
            .iter()
            .find(|error| error.code == NoteValidationCode::UnsupportedCitationStyle.as_str())
            .expect("引用スタイルの診断");
        // 属性名の位置を示す。`= Note\n:`までが8バイトで、そこから属性名が始まる。
        assert_eq!(
            rejected.span,
            Some(Utf8ByteSpan {
                start: 8,
                end: 8 + CITATION_STYLE_DOCUMENT_ATTRIBUTE.len() as u32,
            })
        );
    }

    /// 属性を書かないノートは既定のスタイルになる。
    #[test]
    fn a_note_without_the_attribute_uses_the_default_style() {
        assert_eq!(
            citation_style("= Note\n\n本文 cite:[smith2024]").expect("style"),
            CitationStyle::default()
        );
        assert_eq!(
            citation_style(&format!(
                "= Note\n:{CITATION_STYLE_DOCUMENT_ATTRIBUTE}: numeric\n\n本文"
            ))
            .expect("style"),
            CitationStyle::Numeric
        );
    }

    /// 接頭辞を付ける前の`tags`は、他の未対応属性と同じく拒否する。
    ///
    /// 受理し続けると同じ意味の属性が二つある状態になり、どちらを書くべきかが決まらない。
    #[test]
    fn the_unprefixed_tags_attribute_is_rejected() {
        let errors = validate_draft(NoteDraft {
            source: "= Note\n:tags: 研究\n\n本文".into(),
            title: String::new(),
            tags: Vec::new(),
        })
        .expect_err("接頭辞の無いtags");
        assert!(
            errors.iter().any(
                |error| error.code == NoteValidationCode::UnsupportedDocumentAttribute.as_str()
            )
        );
    }

    /// 条件分岐と取り込みのdirectiveは、どちらの属性記法でも受理しない。
    ///
    /// Marginalisは1件のノートを1つの文書として扱うため、どちらも受理しない。AdocWeave
    /// 0.26.0までは`ifeval::`が名前付きマクロとして読まれ、許可しないURL schemeとして
    /// 拒否されていた。0.27.0で字句として認識されるようになり、既定では警告も出なくなった
    /// ため、`unprocessed-directive`の規則を明示的に有効にしている。この試験は、その設定が
    /// 外れたときに受理範囲が広がることを検出する。
    #[test]
    fn preprocessor_directives_stay_rejected() {
        for source in [
            "= Note\n\ninclude::other.adoc[]",
            "= Note\n:source-language: rust\n\nifeval::[\"{source-language}\" == \"rust\"]\n本文\nendif::[]",
            "= Note\n:source-language: rust\n\nifeval::[\"\\{source-language}\" == \"rust\"]\n本文\nendif::[]",
            "= Note\n:source-language: rust\n\nifdef::source-language[]\n本文\nendif::[]",
            "= Note\n\nifndef::missing[]\n本文\nendif::[]",
        ] {
            let result = validate_draft(NoteDraft {
                source: source.into(),
                title: String::new(),
                tags: Vec::new(),
            });
            let Err(errors) = result else {
                panic!("directiveを受理してしまいました: {source}");
            };
            // 理由が「AsciiDocとして読めない」だと、書いた人は直し方が分からない。
            assert!(
                errors.iter().any(|error| {
                    error.code == NoteValidationCode::PreprocessorDirectiveDisabled.as_str()
                        || error.code == NoteValidationCode::IncludeDirectiveDisabled.as_str()
                }),
                "directiveであることが診断から分かりません: {source} → {:?}",
                errors.iter().map(|error| &error.code).collect::<Vec<_>>()
            );
        }
    }

    /// 本文中の`\{name}`は属性の展開を打ち消した文字列として受理する。
    ///
    /// 0.25.0が変えたのはdirectiveの中の解釈だけで、本文の扱いは以前と同じである。
    #[test]
    fn an_escaped_attribute_reference_in_the_body_is_accepted() {
        let draft = validated("= Note\n:source-language: rust\n\n本文 \\{source-language} です。");
        assert_eq!(draft.title, "Note");
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

    /// outlineは見出しの階層と行範囲を返し、文書題名を一覧から除く。
    #[test]
    fn outline_reports_hierarchical_line_ranges() {
        let source = concat!(
            "= Title\n",         // 1
            "\n",                // 2
            "== Alpha\n",        // 3
            "\n",                // 4
            "alpha body\n",      // 5
            "\n",                // 6
            "=== Alpha child\n", // 7
            "\n",                // 8
            "child body\n",      // 9
            "\n",                // 10
            "== Beta\n",         // 11
            "\n",                // 12
            "beta body\n",       // 13
        );
        let outline = outline(source).expect("outline");
        assert_eq!(outline.line_count, 13);
        let summary: Vec<(u8, &str, usize, usize)> = outline
            .sections
            .iter()
            .map(|section| {
                (
                    section.level,
                    section.title.as_str(),
                    section.start_line,
                    section.end_line,
                )
            })
            .collect();
        // Alphaの範囲は子節Alpha childを含み、Betaの直前で終わる。
        assert_eq!(
            summary,
            vec![
                (1, "Alpha", 3, 10),
                (2, "Alpha child", 7, 10),
                (1, "Beta", 11, 13),
            ]
        );
    }

    /// 明示した`[#id]`だけをアンカーとして返し、自動生成のIDは返さない。
    #[test]
    fn outline_returns_only_explicit_anchors() {
        let source = "= Title\n\n[#sec-problem]\n== Problem\n\n== Approach\n";
        let outline = outline(source).expect("outline");
        assert_eq!(outline.sections.len(), 2);
        assert_eq!(outline.sections[0].anchor.as_deref(), Some("sec-problem"));
        assert_eq!(outline.sections[1].anchor, None);
    }

    /// 見出しのない文書は空の一覧と総行数だけを返す。
    #[test]
    fn outline_of_a_flat_document_is_empty() {
        let outline = outline("= Title\n\nbody only\n").expect("outline");
        assert!(outline.sections.is_empty());
        assert_eq!(outline.line_count, 3);
    }
}
