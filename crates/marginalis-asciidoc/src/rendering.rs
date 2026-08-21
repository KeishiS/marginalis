use adocweave::output::diagnostics::Severity;
use adocweave::output::html::render_with_inputs;
use adocweave::resolution::{
    CitationSegment, GeneratedBibliography, GeneratedBibliographyEntry, MediaType, RenderInputs,
    ResolutionFailureKind, ResolutionNotice, ResolutionNoticeKind, ResolvedCitation,
    ResolvedReference, ResolvedResource, ResolverFailure,
};
use marginalis_application::{
    NoteBibliographyEntry, NoteCitationResolution, NoteReferenceResolution, NoteRenderInputs,
};
use marginalis_domain::Note;

use crate::RenderError;
use crate::analysis::analyze_valid_source;
use crate::configuration::{html_is_within_output_limits, output_limits, render_policy};

/// 生成した参考文献一覧の節の見出し。
const GENERATED_BIBLIOGRAPHY_TITLE: &str = "参考文献";

pub(crate) fn render_note(
    note: &Note,
    inputs: NoteRenderInputs<'_>,
) -> Result<String, RenderError> {
    let analysis = analyze_valid_source(note.source())?;
    let queries = analysis.reference_queries();
    let references = inputs
        .references
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
    let citations = analysis.citations();
    let resolved_citations = inputs
        .citations
        .iter()
        .map(|resolution| {
            let citation = citations
                .get(resolution.citation_index)
                .ok_or(RenderError)?;
            Ok(ResolvedCitation::resolved(
                citation.range,
                citation_segments(resolution),
            ))
        })
        .collect::<Result<Vec<_>, RenderError>>()?;
    let resource_queries = analysis.resource_queries();
    let resources = inputs
        .attachments
        .iter()
        .map(|resolution| {
            let query = resource_queries
                .get(resolution.attachment_index)
                .ok_or(RenderError)?;
            let media_type =
                MediaType::parse(resolution.media_type.as_str()).map_err(|_| RenderError)?;
            Ok(ResolvedResource::resolved(
                query.reference.range(),
                &resolution.href,
                media_type,
                Some(u64::try_from(resolution.byte_length).map_err(|_| RenderError)?),
            ))
        })
        .collect::<Result<Vec<_>, RenderError>>()?;
    let mut render_inputs = RenderInputs::default()
        .with_references(references)
        .with_resources(resources)
        .with_citations(resolved_citations);
    if let Some(bibliography) = generated_bibliography(&analysis, inputs.bibliography) {
        render_inputs = render_inputs.with_generated_bibliography(bibliography);
    }
    let output = render_with_inputs(analysis.document(), &render_policy(), &render_inputs);
    if output
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == Severity::Error)
        || !html_is_within_output_limits(&output.html, &output_limits())
    {
        return Err(RenderError);
    }
    Ok(output.html)
}

fn citation_segments(resolution: &NoteCitationResolution) -> Vec<CitationSegment> {
    resolution
        .segments
        .iter()
        .map(|segment| match &segment.anchor {
            Some(anchor) => CitationSegment::linked(&segment.text, anchor),
            None => CitationSegment::text(&segment.text),
        })
        .collect()
}

/// 引用済みの文献項目から、AdocWeaveへ渡す構造化入力を組み立てる。
///
/// 本文が同じcitation keyの項目を既に定義している場合は、生成した項目を重ねない。
/// 同じanchorが二つあると文書として成り立たず、著者が書いた記述を優先すべきためである。
fn generated_bibliography(
    analysis: &adocweave::Analysis,
    entries: &[NoteBibliographyEntry],
) -> Option<GeneratedBibliography> {
    let defined = analysis
        .catalogs()
        .bibliography()
        .iter()
        .map(|entry| entry.id.as_str())
        .collect::<Vec<_>>();
    let remaining = entries
        .iter()
        .filter(|entry| !defined.contains(&entry.citation_key.as_str()))
        .collect::<Vec<_>>();
    if remaining.is_empty() {
        return None;
    }
    let numbered = numbers_count_up_from_one(&remaining);
    let generated = remaining
        .iter()
        .map(|entry| {
            let generated = GeneratedBibliographyEntry::new(&entry.citation_key, &entry.text);
            match entry.number {
                Some(number) if numbered => generated.with_number(number),
                _ => generated,
            }
        })
        .collect::<Vec<_>>();
    Some(GeneratedBibliography::new(
        GENERATED_BIBLIOGRAPHY_TITLE,
        generated,
    ))
}

/// 一覧へ残った項目が、並び順どおりに1、2、…、nの番号を持つかどうか。
///
/// AdocWeaveは番号を一覧全体の性質として扱い、この条件を満たさない入力を誤りとして報告し、
/// 参考文献一覧を出力しない。番号の付いた一覧は本文の引用と番号で対応するため、番号が飛べば
/// 読み手を別の項目へ導いてしまうからである。
///
/// 番号は引用を解決した時点で決まるが、本文が同じcitation keyを定義している項目はここで
/// 一覧から外れるため、残った番号が飛ぶことがある。その場合は番号を渡さず、番号のない一覧
/// として描画する。本文の引用は解決したときの番号を表示したままになるが、一覧を出せずに
/// ノート全体の描画へ失敗するよりも、読める結果を残すほうがよい。
fn numbers_count_up_from_one(entries: &[&NoteBibliographyEntry]) -> bool {
    entries.iter().enumerate().all(|(position, entry)| {
        u32::try_from(position + 1).is_ok_and(|expected| entry.number == Some(expected))
    })
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use marginalis_application::{NoteAttachmentResolution, NoteCitationSegment};
    use marginalis_domain::{
        EntityId, Identity, Note, NoteCreationSource, NoteDraft, NoteId, NoteRestore,
        NoteReviewTracking, PrincipalId, PrincipalRef, Revision, UnixMillis,
    };

    use super::*;

    fn owner() -> PrincipalRef {
        PrincipalRef::new(
            PrincipalId::new(1).expect("ID"),
            Identity::new("https://id.example.test".into(), "alice".into()).expect("owner"),
        )
    }

    fn note(body: &str) -> Note {
        Note::restore(NoteRestore {
            note_id: NoteId::new(
                EntityId::from_str("0197c9bc-0000-7000-8000-000000000001").expect("UUIDv7"),
            ),
            owner: owner(),
            draft: NoteDraft {
                title: "A title".into(),
                source: format!("= A title\n\n{body}"),
                tags: Vec::new(),
            },
            created_at: UnixMillis::new(0),
            updated_at: UnixMillis::new(1),
            revision: Revision::INITIAL,
            deleted_at: None,
            created_via: NoteCreationSource::Unknown,
            review: NoteReviewTracking::Unknown,
        })
        .expect("note")
    }

    /// 定義リストは、用語の次の行に書いた説明文も同じ行に書いた場合と同様に説明として扱う。
    /// AdocWeave v0.40.1より前は説明文がリスト外の段落になり、`dd`が空になっていた(#482)。
    #[test]
    fn description_lists_attach_descriptions_written_on_the_following_line() {
        let html = render_note(
            &note("用語A::\n次の行に書いた説明です。\n用語B:: 同じ行に書いた説明です。"),
            NoteRenderInputs::default(),
        )
        .expect("render");
        assert!(html.contains("<dd>次の行に書いた説明です。</dd>"));
        assert!(html.contains("<dd>同じ行に書いた説明です。</dd>"));
        assert!(!html.contains("<dd></dd>"));
        assert_eq!(html.matches("<dl>").count(), 1);
    }

    #[test]
    fn supported_blocks_render_without_raw_markup() {
        let html = render_note(
            &note("[[local]]\nA *safe* paragraph. See <<local>>.\n\n[source,python]\n----\nprint(\"hello\")\n----"),
            NoteRenderInputs::default(),
        )
        .expect("render");
        assert!(html.contains("<strong>safe</strong>"));
        assert!(html.contains("language-python"));
        assert!(html.contains("href=\"#local\""));
    }

    #[test]
    fn an_internal_attachment_renders_only_with_the_resolved_same_origin_url() {
        let attachment_id = "0197c9bc-0000-7000-8000-0000000000a1";
        let html = render_note(
            &note(&format!("image::attachment:{attachment_id}[実験結果]")),
            NoteRenderInputs {
                attachments: &[NoteAttachmentResolution {
                    attachment_index: 0,
                    href: format!(
                        "/api/v3/notes/0197c9bc-0000-7000-8000-000000000001/attachments/{attachment_id}/content"
                    ),
                    media_type: marginalis_domain::AttachmentMediaType::Png,
                    byte_length: 32,
                }],
                ..Default::default()
            },
        )
        .expect("render an authorized attachment");

        assert!(html.contains(&format!("attachments/{attachment_id}/content")));
        assert!(html.contains("alt=\"実験結果\""));
        assert!(!html.contains("attachment:"));
        assert!(!html.contains("http://"));
        assert!(!html.contains("https://"));
    }

    #[test]
    fn multiline_list_items_preserve_wrapping_hard_breaks_and_note_references() {
        let target = "0197c9bc-0000-7000-8000-000000000002";
        let html = render_note(
            &note(&format!(
                "* 最初の行 +\n続きの行\n* 参照\n  xref:note:{target}[]\n\n. 一つ目\n  折り返し\n. 二つ目"
            )),
            NoteRenderInputs {
                references: &[NoteReferenceResolution::Visible {
                    reference_index: 0,
                    href: format!("/notes/{target}"),
                    title: "参照先".into(),
                    missing_anchor: false,
                }],
                ..Default::default()
            },
        )
        .expect("render multiline lists");

        assert!(html.contains("<li>最初の行<br>\n続きの行</li>"));
        assert!(html.contains("<li>参照   <a href=\""));
        assert!(html.contains(">参照先</a></li>"));
        assert!(html.contains("<li>一つ目   折り返し</li>"));
        assert!(html.contains("<li>二つ目</li>"));
    }

    #[test]
    fn published_multiline_list_example_is_accepted_and_rendered() {
        let example = crate::policy::note_profile()
            .examples
            .into_iter()
            .find(|example| example.kind == "multiline_list_item")
            .expect("published multiline list example");
        let html = render_note(&note(example.body), NoteRenderInputs::default())
            .expect("render published example");

        assert!(html.contains("<li>First line<br>\nContinued line</li>"));
        assert!(html.contains("<li>Next item</li>"));
    }

    #[test]
    fn source_and_math_html_use_the_public_adocweave_contract() {
        let html = render_note(
            &note(
                ".Example <source>\n[source,rust,linenums,start=7]\n----\nfn main() {}\n----\n\nInline latexmath:[x < y].\n\n[latexmath]\n++++\nx^2 < y\n++++",
            ),
            NoteRenderInputs::default(),
        )
        .expect("render");

        assert!(html.contains("<figure class=\"source-block\">"));
        assert!(html.contains("<figcaption>Example &lt;source&gt;</figcaption>"));
        assert!(html.contains(
            "<pre data-language=\"rust\" data-line-numbers=\"true\" data-line-start=\"7\"><code class=\"language-rust\">fn main() {}"
        ));
        assert!(html.contains(
            "<code class=\"math-latex\" data-math-language=\"latexmath\" data-math-display=\"inline\">x &lt; y</code>"
        ));
        assert!(html.contains(
            "<pre class=\"math-latex\" data-math-language=\"latexmath\" data-math-display=\"block\"><code>x^2 &lt; y"
        ));
        assert!(!html.contains("<source>"));
        assert!(!html.contains("x < y"));
    }

    #[test]
    fn authored_block_roles_are_not_exposed_as_html_classes() {
        let html = render_note(
            &note(".Example\n[.private]\n====\nbody\n===="),
            NoteRenderInputs::default(),
        )
        .expect("render");

        assert!(html.contains("body"));
        assert!(!html.contains("role-private"));
    }

    /// 題を持つlisting blockは、題をcaptionとして描画に残す。
    #[test]
    fn a_titled_listing_block_keeps_its_title() {
        let html = render_note(
            &note(".用例\n----\nfn main() {}\n----"),
            NoteRenderInputs::default(),
        )
        .expect("render");

        assert!(html.contains("<figcaption"), "html: {html}");
        assert!(html.contains("用例"), "html: {html}");
        assert!(html.contains("fn main() {}"));
    }

    #[test]
    fn published_bibliography_example_validates_and_renders_bidirectional_links() {
        let profile = crate::policy::note_profile();
        assert!(profile.syntax.common_blocks.contains(&"bibliography"));
        assert!(
            profile
                .syntax
                .common_inlines
                .contains(&"bibliography_anchor")
        );
        assert!(
            profile
                .syntax
                .common_inlines
                .contains(&"bibliography_reference")
        );
        assert!(
            profile
                .authoring_guidance
                .iter()
                .any(|guidance| guidance.contains("Never invent or infer"))
        );
        let example = profile
            .examples
            .iter()
            .find(|example| example.kind == "bibliography")
            .expect("published bibliography example");
        assert!(example.body.contains("[bibliography]"));
        assert!(example.body.contains("[[[smith2024]]]"));
        assert!(example.body.contains("<<smith2024>>"));
        let draft = crate::validate_note_draft(NoteDraft {
            source: example.body.into(),
            title: String::new(),
            tags: Vec::new(),
        })
        .expect("the published example must be accepted by create_note")
        .draft;
        assert_eq!(draft.title, "先行研究の整理");
        assert_eq!(draft.tags, ["文献", "研究"]);

        let rendered_note = Note::restore(NoteRestore {
            note_id: NoteId::new(
                EntityId::from_str("0197c9bc-0000-7000-8000-000000000003").expect("UUIDv7"),
            ),
            owner: owner(),
            draft,
            created_at: UnixMillis::new(0),
            updated_at: UnixMillis::new(1),
            revision: Revision::INITIAL,
            deleted_at: None,
            created_via: NoteCreationSource::Unknown,
            review: NoteReviewTracking::Unknown,
        })
        .expect("validated note");
        let html = render_note(&rendered_note, NoteRenderInputs::default())
            .expect("render bibliography example");

        assert!(html.contains("href=\"#smith2024\""));
        assert!(html.contains("id=\"smith2024\" class=\"bibliography-anchor\""));
        assert!(html.contains("id=\"_bibliography_ref_"));
        assert!(html.contains("class=\"bibliography-backref\""));
        assert!(html.contains("href=\"#_bibliography_ref_"));
    }

    #[test]
    fn resolved_and_hidden_note_references_preserve_acl_decisions() {
        let target = "0197c9bc-0000-7000-8000-000000000002";
        let source = note(&format!("xref:note:{target}[]"));
        let visible = render_note(
            &source,
            NoteRenderInputs {
                references: &[NoteReferenceResolution::Visible {
                    reference_index: 0,
                    href: format!("/notes/{target}"),
                    title: "参照先".into(),
                    missing_anchor: false,
                }],
                ..Default::default()
            },
        )
        .expect("visible");
        assert!(visible.contains(">参照先</a>"));

        let hidden = render_note(
            &source,
            NoteRenderInputs {
                references: &[NoteReferenceResolution::Hidden { reference_index: 0 }],
                ..Default::default()
            },
        )
        .expect("hidden");
        assert!(!hidden.contains("href="));
        assert!(!hidden.contains(target));
    }

    /// 解決した引用を、生成した参考文献項目と相互にlinkさせる。
    #[test]
    fn resolved_citations_link_to_the_generated_bibliography() {
        let html = render_note(
            &note("結果は cite:[smith2024] と、再度 cite:[smith2024] で報告されています。"),
            NoteRenderInputs {
                citations: &[
                    resolution(0, "smith2024", "Smith 2024"),
                    resolution(1, "smith2024", "Smith 2024"),
                ],
                bibliography: &[NoteBibliographyEntry {
                    citation_key: "smith2024".into(),
                    text: "Smith, A. (2024). An Example Article.".into(),
                    number: None,
                }],
                ..Default::default()
            },
        )
        .expect("render resolved citations");

        assert!(html.contains("参考文献"));
        // 同じ文献を2回引用しても項目は1つで、本文からその項目へ移動できる。
        assert_eq!(html.matches("An Example Article").count(), 1);
        assert_eq!(html.matches("href=\"#smith2024\"").count(), 2);
        assert!(html.contains(">(</span>") || html.contains("(<a"));
        assert!(html.contains(">Smith 2024</a>"));
        // 項目側からは、本文中のそれぞれの引用位置へ戻れる。
        assert_eq!(html.matches("class=\"bibliography-backref\"").count(), 2);
        assert!(!html.contains(">smith2024<"));
    }

    /// 番号を持つ項目は番号付きの一覧として並べ、citation keyをlink先のIDとして維持する。
    #[test]
    fn numbered_entries_are_rendered_as_an_ordered_list() {
        let html = render_note(
            &note("結果は cite:[smith2024] で報告されています。\n追試も cite:[tanaka2025] で行われました。"),
            NoteRenderInputs {
                citations: &[
                    resolution(0, "smith2024", "1"),
                    resolution(1, "tanaka2025", "2"),
                ],
                bibliography: &[
                    NoteBibliographyEntry {
                        citation_key: "smith2024".into(),
                        text: "Smith, A. (2024). An Example Article.".into(),
                        number: Some(1),
                    },
                    NoteBibliographyEntry {
                        citation_key: "tanaka2025".into(),
                        text: "田中 (2025). 追試の報告.".into(),
                        number: Some(2),
                    },
                ],
                ..Default::default()
            },
        )
        .expect("render numbered citations");

        // 一覧が番号付きになり、本文の番号順に項目が並ぶ。
        assert!(html.contains("<ol>"));
        assert!(!html.contains("<ul>"));
        let first = html.find("An Example Article").expect("first entry");
        let second = html.find("追試の報告").expect("second entry");
        assert!(first < second);
        // IDはcitation keyのままで、本文と項目を相互にたどれる。
        assert!(html.contains("id=\"smith2024\""));
        assert!(html.contains("id=\"tanaka2025\""));
        assert_eq!(html.matches("href=\"#smith2024\"").count(), 1);
        assert_eq!(html.matches("class=\"bibliography-backref\"").count(), 2);
        // 番号は文献情報の記述には混ざらない。
        assert!(!html.contains("[1] Smith"));
        assert!(!html.contains("1. Smith"));
    }

    /// 番号を持たない項目は、従来どおり番号のない一覧として並べる。
    #[test]
    fn entries_without_a_number_stay_an_unordered_list() {
        let html = render_note(
            &note("結果は cite:[smith2024] で報告されています。"),
            NoteRenderInputs {
                citations: &[resolution(0, "smith2024", "Smith 2024")],
                bibliography: &[NoteBibliographyEntry {
                    citation_key: "smith2024".into(),
                    text: "Smith, A. (2024). An Example Article.".into(),
                    number: None,
                }],
                ..Default::default()
            },
        )
        .expect("render an entry without a number");

        assert!(html.contains("<ul>"));
        assert!(!html.contains("<ol>"));
        assert!(html.contains("An Example Article"));
    }

    /// 本文が定義した項目を除いた結果、番号が飛ぶ場合は番号を渡さない。
    ///
    /// AdocWeaveは飛んだ番号を誤りとして報告し、参考文献一覧を出力しない。描画に失敗させず、
    /// 番号のない一覧として残りの項目を見せる。
    #[test]
    fn a_gap_left_by_a_document_defined_entry_falls_back_to_an_unordered_list() {
        let html = render_note(
            &note(
                "先行研究 cite:[smith2024] と cite:[tanaka2025] を見ます。\n\n[bibliography]\n== 出典\n\n* [[[smith2024]]] 著者が書いた記述",
            ),
            NoteRenderInputs {
                citations: &[
                    resolution(0, "smith2024", "1"),
                    resolution(1, "tanaka2025", "2"),
                ],
                bibliography: &[
                    NoteBibliographyEntry {
                        citation_key: "smith2024".into(),
                        text: "Smith, A. (2024). An Example Article.".into(),
                        number: Some(1),
                    },
                    NoteBibliographyEntry {
                        citation_key: "tanaka2025".into(),
                        text: "田中 (2025). 追試の報告.".into(),
                        number: Some(2),
                    },
                ],
                ..Default::default()
            },
        )
        .expect("render a bibliography whose numbers do not count up from one");

        // 本文が定義した項目は著者の記述のまま残り、生成した項目は重ならない。
        assert!(html.contains("著者が書いた記述"));
        assert!(!html.contains("An Example Article"));
        // 残った項目は番号のない一覧として並ぶ。
        assert!(html.contains("追試の報告"));
        assert!(html.contains("<ul>"));
        assert!(!html.contains("<ol>"));
    }

    /// 本文が同じcitation keyを定義している場合は、生成した項目を重ねない。
    #[test]
    fn a_document_defined_entry_is_not_generated_twice() {
        let html = render_note(
            &note(
                "結果は cite:[smith2024] です。\n\n[bibliography]\n== 出典\n\n* [[[smith2024]]] 著者が書いた記述",
            ),
            NoteRenderInputs {
                citations: &[resolution(0, "smith2024", "Smith 2024")],
                bibliography: &[NoteBibliographyEntry {
                    citation_key: "smith2024".into(),
                    text: "Smith, A. (2024). An Example Article.".into(),
                    number: None,
                }],
                ..Default::default()
            },
        )
        .expect("render with a document-defined entry");

        assert!(html.contains("著者が書いた記述"));
        assert!(!html.contains("An Example Article"));
        assert!(!html.contains(GENERATED_BIBLIOGRAPHY_TITLE));
        assert!(html.contains("href=\"#smith2024\""));
    }

    /// 文献情報の文字列は表示であり、AsciiDocの記法として解釈しない。
    #[test]
    fn bibliography_text_is_shown_as_written_and_never_as_markup() {
        let html = render_note(
            &note("結果は cite:[trick] です。"),
            NoteRenderInputs {
                citations: &[resolution(0, "trick", "Author 2024")],
                bibliography: &[NoteBibliographyEntry {
                    citation_key: "trick".into(),
                    text: "Effective C++ and More Effective C++; *強調* image:secret[] <<other>> {attribute} pass:[x] +x+ ++x++ +++x+++ <b>&".into(),
                    number: None,
                }],
                ..Default::default()
            },
        )
        .expect("render an entry that contains markup characters");

        assert!(html.contains("Effective C++ and More Effective C++; *強調* image:secret[] &lt;&lt;other&gt;&gt; {attribute} pass:[x] +x+ ++x++ +++x+++ &lt;b&gt;&amp;"));
        assert!(!html.contains("<strong>"));
        assert!(!html.contains("<img"));
    }

    /// DOIやURLは、linkではなく読める文字列として並べる。
    ///
    /// 文献情報を記法として解釈しない方針の結果であり、逆斜線は表示に残らない。
    #[test]
    fn an_address_in_an_entry_stays_readable_without_becoming_a_link() {
        let html = render_note(
            &note("結果は cite:[doi2022] です。"),
            NoteRenderInputs {
                citations: &[resolution(0, "doi2022", "Smith 2022")],
                bibliography: &[NoteBibliographyEntry {
                    citation_key: "doi2022".into(),
                    text: "Smith, A. (2022). Example. https://doi.org/10.1234/example.".into(),
                    number: None,
                }],
                ..Default::default()
            },
        )
        .expect("render an entry that contains an address");

        assert!(html.contains("https://doi.org/10.1234/example."));
        assert!(!html.contains("\\"));
        assert!(!html.contains("href=\"https://doi.org"));
    }

    fn resolution(citation_index: usize, anchor: &str, label: &str) -> NoteCitationResolution {
        NoteCitationResolution {
            citation_index,
            segments: vec![
                NoteCitationSegment {
                    text: "(".into(),
                    anchor: None,
                },
                NoteCitationSegment {
                    text: label.into(),
                    anchor: Some(anchor.into()),
                },
                NoteCitationSegment {
                    text: ")".into(),
                    anchor: None,
                },
            ],
        }
    }

    #[test]
    fn invalid_source_is_rejected_before_rendering() {
        for body in [
            "include::secret[]",
            "xref:note:not-a-note[invalid note reference]",
        ] {
            assert_eq!(
                render_note(&note(body), NoteRenderInputs::default()),
                Err(RenderError),
                "{body}"
            );
        }
    }
}
