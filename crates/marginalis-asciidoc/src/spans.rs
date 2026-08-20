//! AdocWeaveの意味モデルから、編集画面の装飾に使うspan注釈を導く。
//!
//! 装飾の判断に必要な種類と範囲だけを写し、地の文や装飾しない記法は返さない。
//! 範囲の数え方は診断と同じ、原文のUTF-8バイトオフセットである。

use adocweave::semantic::{
    Block, DelimitedBlockKind, DelimitedPresentation, HeadingKind, Inline, InlineLiteralKind,
    InlineStyle, SemanticNode, StandardMacroKind, VerbatimKind, walk,
};
use adocweave::text::TextRange;
use marginalis_application::{NoteSourceSpan, NoteSourceSpanKind};
use marginalis_domain::Utf8ByteSpan;

use crate::policy::span;

pub(crate) fn source_spans_from_analysis(analysis: &adocweave::Analysis) -> Vec<NoteSourceSpan> {
    let mut spans = Vec::new();
    walk(analysis.document(), |node| match node {
        SemanticNode::Block(block) => collect_block(block, &mut spans),
        SemanticNode::ListItem(item) => spans.push(NoteSourceSpan {
            kind: NoteSourceSpanKind::ListItem,
            span: span(item.range),
            content_span: Some(span(item.text_range)),
            marker_spans: non_empty_spans([Some(item.marker_range), Some(item.separator_range)]),
            level: None,
        }),
        SemanticNode::Inline(inline) => collect_inline(inline, &mut spans),
        SemanticNode::Attribute(occurrence) => spans.push(NoteSourceSpan {
            kind: NoteSourceSpanKind::DocumentAttribute,
            span: span(occurrence.range),
            content_span: None,
            marker_spans: Vec::new(),
            level: None,
        }),
        SemanticNode::Anchor(anchor) => spans.push(NoteSourceSpan {
            kind: NoteSourceSpanKind::Anchor,
            span: span(anchor.range),
            content_span: None,
            marker_spans: Vec::new(),
            level: None,
        }),
        _ => {}
    });
    // 出現順に整え、同じ開始位置では外側の記法を先にする。
    spans.sort_by_key(|item| (item.span.start, std::cmp::Reverse(item.span.end)));
    spans
}

fn collect_block(block: &Block, spans: &mut Vec<NoteSourceSpan>) {
    match block {
        Block::Heading(heading) => {
            let (kind, level) = match heading.kind {
                HeadingKind::DocumentTitle => (NoteSourceSpanKind::DocumentTitle, None),
                HeadingKind::Part => (NoteSourceSpanKind::Heading, Some(0)),
                HeadingKind::Section { level } | HeadingKind::Discrete { level } => {
                    (NoteSourceSpanKind::Heading, Some(level))
                }
            };
            spans.push(NoteSourceSpan {
                kind,
                span: span(heading.range),
                content_span: Some(span(heading.text_range)),
                marker_spans: non_empty_spans([
                    Some(heading.marker_range),
                    Some(heading.separator_range),
                ]),
                level,
            });
        }
        Block::Paragraph(paragraph) => {
            if let Some(admonition) = &paragraph.admonition {
                spans.push(NoteSourceSpan {
                    kind: NoteSourceSpanKind::Admonition,
                    span: span(paragraph.range),
                    content_span: Some(span(paragraph.content_range)),
                    marker_spans: non_empty_spans([Some(admonition.label_range)]),
                    level: None,
                });
            }
        }
        Block::LiteralParagraph(literal) => spans.push(NoteSourceSpan {
            kind: NoteSourceSpanKind::LiteralBlock,
            span: span(literal.range),
            content_span: Some(span(literal.content_range)),
            marker_spans: Vec::new(),
            level: None,
        }),
        Block::Source(source) => spans.push(NoteSourceSpan {
            kind: NoteSourceSpanKind::SourceBlock,
            span: span(source.range),
            content_span: Some(span(source.content_range)),
            marker_spans: non_empty_spans([
                Some(source.attribute_range),
                Some(source.delimiter_range),
            ]),
            level: None,
        }),
        Block::Verbatim(verbatim) => spans.push(NoteSourceSpan {
            kind: match verbatim.kind {
                VerbatimKind::Source(_) => NoteSourceSpanKind::SourceBlock,
                VerbatimKind::Listing | VerbatimKind::Literal => NoteSourceSpanKind::LiteralBlock,
            },
            span: span(verbatim.range),
            content_span: Some(span(verbatim.content_range)),
            marker_spans: non_empty_spans([Some(verbatim.delimiter_range)]),
            level: None,
        }),
        Block::Math(math) => spans.push(NoteSourceSpan {
            kind: NoteSourceSpanKind::MathBlock,
            span: span(math.range),
            content_span: Some(span(math.content_range)),
            marker_spans: non_empty_spans([Some(math.attribute_range), Some(math.delimiter_range)]),
            level: None,
        }),
        Block::Delimited(delimited) => {
            let kind = match (&delimited.presentation, delimited.kind) {
                (Some(DelimitedPresentation::Admonition(_)), _) => {
                    Some(NoteSourceSpanKind::Admonition)
                }
                (Some(DelimitedPresentation::Quote(_)), _) | (_, DelimitedBlockKind::Quote) => {
                    Some(NoteSourceSpanKind::Quote)
                }
                (_, DelimitedBlockKind::Example) => Some(NoteSourceSpanKind::Example),
                (_, DelimitedBlockKind::Literal | DelimitedBlockKind::Listing) => {
                    Some(NoteSourceSpanKind::LiteralBlock)
                }
                (_, DelimitedBlockKind::Table) => Some(NoteSourceSpanKind::Table),
                (
                    _,
                    DelimitedBlockKind::Comment
                    | DelimitedBlockKind::Open
                    | DelimitedBlockKind::Sidebar
                    | DelimitedBlockKind::Pass,
                ) => None,
            };
            if let Some(kind) = kind {
                let label = match &delimited.presentation {
                    Some(DelimitedPresentation::Admonition(admonition)) => {
                        Some(admonition.label_range)
                    }
                    _ => None,
                };
                spans.push(NoteSourceSpan {
                    kind,
                    span: span(delimited.range),
                    content_span: Some(span(delimited.content_range)),
                    marker_spans: non_empty_spans([
                        label,
                        Some(delimited.opening_delimiter_range),
                        delimited.closing_delimiter_range,
                    ]),
                    level: None,
                });
            }
        }
        Block::List(_) | Block::Break(_) | Block::Unsupported(_) => {}
    }
}

fn collect_inline(inline: &Inline, spans: &mut Vec<NoteSourceSpan>) {
    match inline {
        Inline::Styled {
            style,
            range,
            content_range,
            ..
        } => {
            let kind = match style {
                InlineStyle::Strong => Some(NoteSourceSpanKind::Strong),
                InlineStyle::Emphasis => Some(NoteSourceSpanKind::Emphasis),
                InlineStyle::Highlight => Some(NoteSourceSpanKind::Highlight),
                InlineStyle::Subscript => Some(NoteSourceSpanKind::Subscript),
                InlineStyle::Superscript => Some(NoteSourceSpanKind::Superscript),
                // 引用符の置き換え表示は行わないため、装飾の対象にしない。
                InlineStyle::CurvedDoubleQuote | InlineStyle::CurvedSingleQuote => None,
            };
            if let Some(kind) = kind {
                spans.push(delimited_inline(kind, *range, *content_range));
            }
        }
        Inline::Literal {
            kind: InlineLiteralKind::Monospace,
            range,
            content_range,
            ..
        } => spans.push(delimited_inline(
            NoteSourceSpanKind::Monospace,
            *range,
            *content_range,
        )),
        Inline::Formula(formula) => spans.push(delimited_inline(
            NoteSourceSpanKind::InlineMath,
            formula.range,
            formula.content_range,
        )),
        Inline::Link(link) => spans.push(labeled_inline(
            NoteSourceSpanKind::Link,
            link.range,
            link.label_range,
            link.target_range,
        )),
        Inline::Reference(reference) => spans.push(labeled_inline(
            NoteSourceSpanKind::CrossReference,
            reference.range,
            reference.label_range,
            reference.target_range,
        )),
        Inline::Macro(node) if node.kind == StandardMacroKind::Citation => {
            spans.push(NoteSourceSpan {
                kind: NoteSourceSpanKind::Citation,
                span: span(node.range),
                content_span: None,
                marker_spans: Vec::new(),
                level: None,
            });
        }
        _ => {}
    }
}

/// 前後を記法文字が挟む、`*強調*`のようなインライン1件を写す。
fn delimited_inline(
    kind: NoteSourceSpanKind,
    range: TextRange,
    content_range: TextRange,
) -> NoteSourceSpan {
    let whole = span(range);
    let content = span(content_range);
    NoteSourceSpan {
        kind,
        span: whole,
        content_span: Some(content),
        marker_spans: non_empty_byte_spans([
            Utf8ByteSpan {
                start: whole.start,
                end: content.start,
            },
            Utf8ByteSpan {
                start: content.end,
                end: whole.end,
            },
        ]),
        level: None,
    }
}

/// 表示文と参照先を持つ、`xref:...[表示文]`のようなインライン1件を写す。
///
/// 表示文があるときはそれ以外を折り畳み、無いときは参照先が表示文を兼ねるため折り畳まない。
fn labeled_inline(
    kind: NoteSourceSpanKind,
    range: TextRange,
    label_range: Option<TextRange>,
    target_range: TextRange,
) -> NoteSourceSpan {
    let whole = span(range);
    match label_range {
        Some(label) => {
            let content = span(label);
            NoteSourceSpan {
                kind,
                span: whole,
                content_span: Some(content),
                marker_spans: non_empty_byte_spans([
                    Utf8ByteSpan {
                        start: whole.start,
                        end: content.start,
                    },
                    Utf8ByteSpan {
                        start: content.end,
                        end: whole.end,
                    },
                ]),
                level: None,
            }
        }
        None => NoteSourceSpan {
            kind,
            span: whole,
            content_span: Some(span(target_range)),
            marker_spans: Vec::new(),
            level: None,
        },
    }
}

fn non_empty_spans<const N: usize>(ranges: [Option<TextRange>; N]) -> Vec<Utf8ByteSpan> {
    non_empty_byte_spans(ranges.into_iter().flatten().map(span))
}

fn non_empty_byte_spans(spans: impl IntoIterator<Item = Utf8ByteSpan>) -> Vec<Utf8ByteSpan> {
    spans
        .into_iter()
        .filter(|span| span.start < span.end)
        .collect()
}

#[cfg(test)]
mod tests {
    use marginalis_application::{NoteSourceSpan, NoteSourceSpanKind};
    use marginalis_domain::{NoteDraft, Utf8ByteSpan};

    fn spans_of(source: &str) -> Vec<NoteSourceSpan> {
        crate::analysis::validate_draft(NoteDraft {
            source: source.into(),
            title: String::new(),
            tags: Vec::new(),
        })
        .expect("valid document")
        .source_spans
    }

    fn slice(source: &str, span: Utf8ByteSpan) -> &str {
        &source[span.start as usize..span.end as usize]
    }

    #[test]
    fn document_structure_is_annotated_in_source_order() {
        let source = "= 題名\n:marginalis-tags: rust\n\n== 見出し\n\n本文です。\n";
        let spans = spans_of(source);
        let kinds: Vec<_> = spans.iter().map(|item| item.kind).collect();
        assert_eq!(
            kinds,
            [
                NoteSourceSpanKind::DocumentTitle,
                NoteSourceSpanKind::DocumentAttribute,
                NoteSourceSpanKind::Heading,
            ]
        );
        let title = &spans[0];
        assert_eq!(slice(source, title.span), "= 題名\n");
        assert_eq!(
            slice(source, title.content_span.expect("題名の本文")),
            "題名"
        );
        assert_eq!(title.level, None);
        let heading = &spans[2];
        assert_eq!(heading.level, Some(1));
        assert_eq!(
            slice(source, heading.content_span.expect("見出しの本文")),
            "見出し"
        );
        let markers: Vec<_> = heading
            .marker_spans
            .iter()
            .map(|span| slice(source, *span))
            .collect();
        assert_eq!(markers, ["==", " "]);
    }

    #[test]
    fn inline_markup_reports_content_and_foldable_markers() {
        // 制約付き記法は、CJK文字の隣接でも認識される(adocweave#576、0.41.0で対応)。
        let source = "= 題名\n\n*太字*と_強調_と`等幅`を含みます。\n";
        let spans = spans_of(source);
        let strong = spans
            .iter()
            .find(|item| item.kind == NoteSourceSpanKind::Strong)
            .expect("strong span");
        assert_eq!(slice(source, strong.span), "*太字*");
        assert_eq!(
            slice(source, strong.content_span.expect("本文部分")),
            "太字"
        );
        let markers: Vec<_> = strong
            .marker_spans
            .iter()
            .map(|span| slice(source, *span))
            .collect();
        assert_eq!(markers, ["*", "*"]);
        assert!(
            spans
                .iter()
                .any(|item| item.kind == NoteSourceSpanKind::Emphasis)
        );
        let monospace = spans
            .iter()
            .find(|item| item.kind == NoteSourceSpanKind::Monospace)
            .expect("monospace span");
        assert_eq!(
            slice(source, monospace.content_span.expect("本文部分")),
            "等幅"
        );
    }

    #[test]
    fn labeled_reference_folds_everything_but_the_label() {
        let source = concat!(
            "= 題名\n\n",
            "先行調査は xref:note:0197c9bc-0000-7000-8000-000000000002[表示文] を参照。\n",
        );
        let spans = spans_of(source);
        let reference = spans
            .iter()
            .find(|item| item.kind == NoteSourceSpanKind::CrossReference)
            .expect("cross reference span");
        assert_eq!(
            slice(source, reference.content_span.expect("表示文")),
            "表示文"
        );
        let folded: String = reference
            .marker_spans
            .iter()
            .map(|span| slice(source, *span))
            .collect();
        assert_eq!(folded, "xref:note:0197c9bc-0000-7000-8000-000000000002[]");
    }

    #[test]
    fn math_citation_and_blocks_are_annotated() {
        let source = concat!(
            "= 題名\n:stem: latexmath\n\n",
            "円の面積は stem:[\\pi r^2] です( cite:[smith2024] を参照)。\n\n",
            "[latexmath]\n++++\nE = mc^2\n++++\n\n",
            "[quote]\n____\n引用文です。\n____\n\n",
            "[source,rust]\n----\nfn main() {}\n----\n",
        );
        let spans = spans_of(source);
        let inline_math = spans
            .iter()
            .find(|item| item.kind == NoteSourceSpanKind::InlineMath)
            .expect("inline math span");
        assert_eq!(
            slice(source, inline_math.content_span.expect("数式本文")),
            "\\pi r^2"
        );
        assert!(
            spans
                .iter()
                .any(|item| item.kind == NoteSourceSpanKind::Citation)
        );
        let math_block = spans
            .iter()
            .find(|item| item.kind == NoteSourceSpanKind::MathBlock)
            .expect("math block span");
        assert_eq!(
            slice(source, math_block.content_span.expect("数式本文")).trim(),
            "E = mc^2"
        );
        assert!(
            spans
                .iter()
                .any(|item| item.kind == NoteSourceSpanKind::Quote)
        );
        let source_block = spans
            .iter()
            .find(|item| item.kind == NoteSourceSpanKind::SourceBlock)
            .expect("source block span");
        assert_eq!(
            slice(source, source_block.content_span.expect("コード本文")).trim(),
            "fn main() {}"
        );
    }

    #[test]
    fn list_items_and_admonitions_are_annotated() {
        let source = concat!(
            "= 題名\n\n",
            "NOTE: 注意書きです。\n\n",
            "* 項目1\n",
            "** 項目2\n",
        );
        let spans = spans_of(source);
        let admonition = spans
            .iter()
            .find(|item| item.kind == NoteSourceSpanKind::Admonition)
            .expect("admonition span");
        let labels: Vec<_> = admonition
            .marker_spans
            .iter()
            .map(|span| slice(source, *span))
            .collect();
        assert!(labels.iter().any(|label| label.starts_with("NOTE")));
        let items: Vec<_> = spans
            .iter()
            .filter(|item| item.kind == NoteSourceSpanKind::ListItem)
            .collect();
        assert_eq!(items.len(), 2);
        let markers: Vec<_> = items
            .iter()
            .map(|item| slice(source, item.marker_spans[0]))
            .collect();
        assert_eq!(markers, ["*", "**"]);
    }

    #[test]
    fn nested_markup_keeps_the_outer_span_first() {
        let source = "= 題名\n\n**__強い強調__**です。\n";
        let spans = spans_of(source);
        let strong_index = spans
            .iter()
            .position(|item| item.kind == NoteSourceSpanKind::Strong)
            .expect("strong span");
        let emphasis_index = spans
            .iter()
            .position(|item| item.kind == NoteSourceSpanKind::Emphasis)
            .expect("emphasis span");
        assert!(strong_index < emphasis_index);
        let starts: Vec<_> = spans.iter().map(|item| item.span.start).collect();
        assert!(starts.is_sorted());
    }
}
