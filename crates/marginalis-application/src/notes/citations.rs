//! 引用の解決と参考文献一覧の組み立て。

use std::collections::HashMap;

use marginalis_domain::{BibliographyItem, Identity, NoteValidationTarget};

use crate::{CitationStyle, NoteAdvisoryDiagnostic, NoteAdvisorySeverity, NoteUseCaseError};

use super::{
    NoteApplication, NoteBibliographyEntry, NoteCitationQuery, NoteCitationResolution,
    NoteCitationSegment, map_owner_resource_error,
};

pub(super) const UNKNOWN_CITATION_KEY_CODE: &str = "unknown_citation_key";
pub(super) const UNKNOWN_CITATION_KEY_DESCRIPTION: &str =
    "a citation key is not registered in the note owner's bibliography library";

impl NoteApplication {
    /// 引用のcitation keyを、ノートを書いた利用者の文献ライブラリで解決する。
    ///
    /// 閲覧者ではなく作成者のライブラリを使うため、同じノートは誰が見ても同じ表示になる。
    /// 解決できたkeyだけが参考文献一覧へ並び、同じ文献を何度引用しても項目は1つになる。
    pub(super) async fn citation_resolutions(
        &self,
        owner: &Identity,
        queries: &[NoteCitationQuery],
        style: CitationStyle,
    ) -> Result<ResolvedCitations, NoteUseCaseError> {
        if queries.is_empty() {
            return Ok(ResolvedCitations::default());
        }
        let mut cited_keys = Vec::new();
        for key in queries.iter().flat_map(|query| query.keys.iter()) {
            if !cited_keys.contains(key) {
                cited_keys.push(key.clone());
            }
        }
        let items = self
            .bibliography
            .items_by_citation_keys(owner, &cited_keys)
            .await
            .map_err(map_owner_resource_error)?;
        let items = items
            .into_iter()
            .map(|item| (item.citation_key().to_owned(), item))
            .collect::<HashMap<_, _>>();

        // 番号で示すスタイルは、本文での初出順に通し番号を振る。解決できたkeyだけが一覧へ
        // 並ぶため、番号も解決できたkeyの中で数える。
        let numbers = cited_keys
            .iter()
            .filter(|key| items.contains_key(*key))
            .enumerate()
            .map(|(position, key)| (key.clone(), position + 1))
            .collect::<HashMap<_, _>>();
        let resolutions = queries
            .iter()
            .map(|query| NoteCitationResolution {
                citation_index: query.citation_index,
                segments: citation_segments(query, &items, &numbers, style),
            })
            .collect();
        let entries = cited_keys
            .iter()
            .filter_map(|key| {
                let item = items.get(key)?;
                Some(NoteBibliographyEntry {
                    citation_key: key.clone(),
                    text: style.entry_text(item),
                    number: style.entry_number(numbers[key]),
                })
            })
            .collect();
        let unknown_keys = cited_keys
            .iter()
            .filter(|key| !items.contains_key(*key))
            .cloned()
            .collect::<Vec<_>>();
        Ok(ResolvedCitations {
            resolutions,
            entries,
            diagnostics: unknown_citation_diagnostics(queries, &unknown_keys),
        })
    }
}

/// 1つのノートについて解決した引用の表示、参考文献項目、保存を妨げない診断。
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct ResolvedCitations {
    pub(super) resolutions: Vec<NoteCitationResolution>,
    pub(super) entries: Vec<NoteBibliographyEntry>,
    pub(super) diagnostics: Vec<NoteAdvisoryDiagnostic>,
}

/// 引用1件の表示を、括弧と区切りを含む文字列の並びへ組み立てる。
///
/// 解決できたkeyは参考文献項目のanchorへlinkし、解決できなかったkeyはcitation keyを
/// そのまま表示する。定義のない`<<key>>`と同じ見え方にそろえ、値を推測しない。
fn citation_segments(
    query: &NoteCitationQuery,
    items: &HashMap<String, BibliographyItem>,
    numbers: &HashMap<String, usize>,
    style: CitationStyle,
) -> Vec<NoteCitationSegment> {
    let (opening, closing) = style.brackets();
    let mut segments = vec![NoteCitationSegment {
        text: opening.into(),
        anchor: None,
    }];
    for (position, key) in query.keys.iter().enumerate() {
        if position > 0 {
            segments.push(NoteCitationSegment {
                text: style.key_separator().into(),
                anchor: None,
            });
        }
        match items.get(key) {
            Some(item) => segments.push(NoteCitationSegment {
                text: style.inline_label(item, numbers[key]),
                anchor: Some(key.clone()),
            }),
            None => segments.push(NoteCitationSegment {
                text: key.clone(),
                anchor: None,
            }),
        }
    }
    let closing = match query.locator.as_deref() {
        Some(locator) => format!(", {locator}{closing}"),
        None => closing.into(),
    };
    segments.push(NoteCitationSegment {
        text: closing,
        anchor: None,
    });
    segments
}

/// 文献ライブラリに無いcitation keyを、保存を妨げない警告として報告する。
fn unknown_citation_diagnostics(
    queries: &[NoteCitationQuery],
    unknown_keys: &[String],
) -> Vec<NoteAdvisoryDiagnostic> {
    queries
        .iter()
        .filter_map(|query| {
            let missing = query
                .keys
                .iter()
                .filter(|key| unknown_keys.contains(key))
                .cloned()
                .collect::<Vec<_>>();
            (!missing.is_empty()).then(|| NoteAdvisoryDiagnostic {
                code: UNKNOWN_CITATION_KEY_CODE.into(),
                severity: NoteAdvisorySeverity::Warning,
                target: NoteValidationTarget::Source,
                span: Some(query.span),
                position: Some(query.position),
                message: format!(
                    "the bibliography library has no item for {}",
                    missing.join(", ")
                ),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use marginalis_domain::{Identity, Utf8ByteSpan};

    use crate::{CitationStyle, NoteAdvisorySeverity};

    use super::*;
    use crate::notes::test_support::{
        AcceptContent, MemoryNotes, NoMathMacros, OneItemLibrary, note_application,
    };

    /// 番号で示すスタイルは、本文での初出順に通し番号を振る。
    ///
    /// 同じ文献を何度引用しても番号は変わらず、参考文献一覧の項目も1つだけになる。番号は
    /// 一覧の項目にも付くため、本文の`[1]`から一覧の`[1]`を探せる。
    #[tokio::test]
    async fn numeric_style_numbers_citations_by_first_appearance() {
        let application = citation_application();
        let queries = vec![
            NoteCitationQuery {
                citation_index: 0,
                keys: vec!["smith2024".into()],
                locator: None,
                span: Utf8ByteSpan { start: 10, end: 30 },
                position: crate::NoteSourcePosition {
                    line: 1,
                    column: 11,
                },
            },
            NoteCitationQuery {
                citation_index: 1,
                keys: vec!["tanaka2025".into()],
                locator: None,
                span: Utf8ByteSpan { start: 40, end: 60 },
                position: crate::NoteSourcePosition {
                    line: 1,
                    column: 41,
                },
            },
            NoteCitationQuery {
                citation_index: 2,
                keys: vec!["smith2024".into()],
                locator: None,
                span: Utf8ByteSpan { start: 70, end: 90 },
                position: crate::NoteSourcePosition {
                    line: 1,
                    column: 71,
                },
            },
        ];

        let resolved = application
            .citation_resolutions(&OneItemLibrary::owner(), &queries, CitationStyle::Numeric)
            .await
            .expect("resolve citations");

        assert_eq!(inline_text(&resolved.resolutions[0]), "[1]");
        assert_eq!(inline_text(&resolved.resolutions[1]), "[2]");
        assert_eq!(inline_text(&resolved.resolutions[2]), "[1]");
        assert_eq!(
            resolved
                .entries
                .iter()
                .map(|entry| entry.citation_key.as_str())
                .collect::<Vec<_>>(),
            vec!["smith2024", "tanaka2025"]
        );
        assert_eq!(resolved.entries[0].number, Some(1));
        assert_eq!(resolved.entries[1].number, Some(2));
        assert!(resolved.entries[0].text.starts_with("Smith"));
    }

    /// 一つの引用が複数のkeyを名指す場合は、番号を読点で並べる。
    #[tokio::test]
    async fn numeric_style_joins_several_keys_in_one_citation() {
        let application = citation_application();
        let queries = vec![NoteCitationQuery {
            citation_index: 0,
            keys: vec!["smith2024".into(), "tanaka2025".into()],
            locator: None,
            span: Utf8ByteSpan { start: 10, end: 40 },
            position: crate::NoteSourcePosition {
                line: 1,
                column: 11,
            },
        }];

        let resolved = application
            .citation_resolutions(&OneItemLibrary::owner(), &queries, CitationStyle::Numeric)
            .await
            .expect("resolve citations");

        assert_eq!(inline_text(&resolved.resolutions[0]), "[1, 2]");
    }

    /// 番号で示すスタイルでも、解決できたkeyは一覧の項目へlinkする。
    #[tokio::test]
    async fn numeric_style_keeps_the_link_to_the_reference_list() {
        let application = citation_application();
        let queries = vec![NoteCitationQuery {
            citation_index: 0,
            keys: vec!["smith2024".into()],
            locator: None,
            span: Utf8ByteSpan { start: 10, end: 30 },
            position: crate::NoteSourcePosition {
                line: 1,
                column: 11,
            },
        }];

        let resolved = application
            .citation_resolutions(&OneItemLibrary::owner(), &queries, CitationStyle::Numeric)
            .await
            .expect("resolve citations");

        let linked = resolved.resolutions[0]
            .segments
            .iter()
            .find(|segment| segment.anchor.is_some())
            .expect("linkする断片");
        assert_eq!(linked.text, "1");
        assert_eq!(linked.anchor.as_deref(), Some("smith2024"));
    }

    /// 引用は指定した所有者のライブラリで解決し、未登録のkeyは警告として報告する。
    #[tokio::test]
    async fn resolves_for_the_named_owner_and_reports_unknown_keys() {
        let application = citation_application();
        let queries = vec![
            NoteCitationQuery {
                citation_index: 0,
                keys: vec!["smith2024".into(), "missing2024".into()],
                locator: Some("p. 12".into()),
                span: Utf8ByteSpan { start: 10, end: 40 },
                position: crate::NoteSourcePosition {
                    line: 1,
                    column: 11,
                },
            },
            NoteCitationQuery {
                citation_index: 1,
                keys: vec!["smith2024".into()],
                locator: None,
                span: Utf8ByteSpan { start: 60, end: 80 },
                position: crate::NoteSourcePosition {
                    line: 1,
                    column: 61,
                },
            },
        ];

        let resolved = application
            .citation_resolutions(&OneItemLibrary::owner(), &queries, CitationStyle::default())
            .await
            .expect("resolve citations");

        assert_eq!(
            resolved.resolutions[0].segments,
            vec![
                NoteCitationSegment {
                    text: "(".into(),
                    anchor: None,
                },
                NoteCitationSegment {
                    text: "Smith 2024".into(),
                    anchor: Some("smith2024".into()),
                },
                NoteCitationSegment {
                    text: "; ".into(),
                    anchor: None,
                },
                NoteCitationSegment {
                    text: "missing2024".into(),
                    anchor: None,
                },
                NoteCitationSegment {
                    text: ", p. 12)".into(),
                    anchor: None,
                },
            ]
        );
        assert_eq!(
            resolved.entries,
            vec![NoteBibliographyEntry {
                citation_key: "smith2024".into(),
                text: "Smith, A. (2024). An Example Article.".into(),
                number: None,
            }]
        );
        assert_eq!(resolved.diagnostics.len(), 1);
        assert_eq!(resolved.diagnostics[0].code, "unknown_citation_key");
        assert_eq!(
            resolved.diagnostics[0].severity,
            NoteAdvisorySeverity::Warning
        );
        assert_eq!(
            resolved.diagnostics[0].span,
            Some(Utf8ByteSpan { start: 10, end: 40 })
        );

        let other = Identity::new("https://id.example.test".into(), "bob".into()).expect("owner");
        let resolved = application
            .citation_resolutions(&other, &queries, CitationStyle::default())
            .await
            .expect("resolve citations for another owner");
        assert!(resolved.entries.is_empty());
        assert_eq!(resolved.diagnostics.len(), 2);
    }

    fn inline_text(resolution: &NoteCitationResolution) -> String {
        resolution
            .segments
            .iter()
            .map(|segment| segment.text.as_str())
            .collect()
    }

    fn citation_application() -> NoteApplication {
        let repository = Arc::new(MemoryNotes::default());
        note_application(
            &repository,
            Arc::new(AcceptContent::default()),
            Arc::new(OneItemLibrary),
            Arc::new(NoMathMacros),
        )
    }
}
