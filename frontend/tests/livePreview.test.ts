import { EditorState, EditorSelection } from "@codemirror/state";
import { Decoration } from "@codemirror/view";
import { describe, expect, it } from "vitest";

import { NoteSourceSpan } from "../src/api";
import {
  livePreview,
  livePreviewField,
  setLiveSpans,
  toLiveSpans,
} from "../src/editor/livePreview";

const byteSpan = (start: number, end: number) => ({
  start,
  end,
  unit: "utf8_byte" as const,
});

/** `**太字**` を含む1行の文書と、そのspan注釈。バイト位置はUTF-8で数える。 */
const source = "**太字**を含む文です。";
const strongSpan: NoteSourceSpan = {
  kind: "strong",
  span: byteSpan(0, 12),
  content_span: byteSpan(2, 8),
  marker_spans: [byteSpan(0, 2), byteSpan(8, 12)],
};

describe("span注釈の内部表現への変換", () => {
  it("UTF-8バイト位置をUTF-16オフセットへ直す", () => {
    const [span] = toLiveSpans(source, [strongSpan]);
    expect(source.slice(span.from, span.to)).toBe("**太字**");
    expect(source.slice(span.contentFrom, span.contentTo)).toBe("太字");
    expect(
      span.markers.map((marker) => source.slice(marker.from, marker.to)),
    ).toEqual(["**", "**"]);
  });

  it("範囲が逆転したspanを捨て、本文を超える位置は末尾へ丸める", () => {
    const inverted: NoteSourceSpan = {
      kind: "strong",
      span: byteSpan(10, 4),
      marker_spans: [],
    };
    const outside: NoteSourceSpan = {
      kind: "strong",
      span: byteSpan(0, 10_000),
      marker_spans: [],
    };
    const spans = toLiveSpans(source, [inverted, outside]);
    expect(spans).toHaveLength(1);
    expect(spans[0].to).toBe(source.length);
  });

  it("content_spanの無い記法は全体を本文部分として扱う", () => {
    const attribute: NoteSourceSpan = {
      kind: "document_attribute",
      span: byteSpan(0, 2),
    };
    const [span] = toLiveSpans(source, [attribute]);
    expect(span.contentFrom).toBe(span.from);
    expect(span.contentTo).toBe(span.to);
  });
});

function decorationSummary(state: EditorState) {
  const summary: { from: number; to: number; kind: string }[] = [];
  const iterator = state.field(livePreviewField).decorations.iter();
  while (iterator.value !== null) {
    const spec = (iterator.value as Decoration).spec as {
      class?: string;
      widget?: unknown;
    };
    summary.push({
      from: iterator.from,
      to: iterator.to,
      kind: spec.class ?? (spec.widget ? "widget" : "replace"),
    });
    iterator.next();
  }
  return summary;
}

function stateWithSpans(doc: string, spans: NoteSourceSpan[]) {
  const state = EditorState.create({
    doc,
    // 既定のカーソル位置0はspanの先端に接触して開示になるため、本文の末尾へ置く。
    selection: EditorSelection.cursor(doc.length),
    extensions: [livePreview({ styleNonce: "test-style-nonce" })],
  });
  return state.update({
    effects: setLiveSpans.of({
      spans: toLiveSpans(doc, spans),
      mathMacros: [],
    }),
  }).state;
}

describe("Live Previewの装飾", () => {
  it("mark装飾とmarkerの折り畳みを組み立てる", () => {
    const state = stateWithSpans(source, [strongSpan]);
    expect(decorationSummary(state)).toEqual([
      { from: 0, to: 2, kind: "replace" },
      { from: 2, to: 4, kind: "lp-strong" },
      { from: 4, to: 6, kind: "replace" },
    ]);
  });

  it("選択範囲が交差した記法はmarkerを開示し、markは維持する", () => {
    const initial = stateWithSpans(source, [strongSpan]);
    const revealed = initial.update({
      selection: EditorSelection.cursor(3),
    }).state;
    expect(decorationSummary(revealed)).toEqual([
      { from: 2, to: 4, kind: "lp-strong" },
    ]);
    const concealed = revealed.update({
      selection: EditorSelection.cursor(10),
    }).state;
    expect(decorationSummary(concealed)).toEqual([
      { from: 0, to: 2, kind: "replace" },
      { from: 2, to: 4, kind: "lp-strong" },
      { from: 4, to: 6, kind: "replace" },
    ]);
  });

  it("本文の変更で装飾を追従させ、次のspan注釈で置き換えられる", () => {
    const initial = stateWithSpans(source, [strongSpan]);
    const inserted = initial.update({
      changes: { from: source.length, insert: "追記" },
      selection: EditorSelection.cursor(source.length + 2),
    }).state;
    expect(decorationSummary(inserted)).toEqual([
      { from: 0, to: 2, kind: "replace" },
      { from: 2, to: 4, kind: "lp-strong" },
      { from: 4, to: 6, kind: "replace" },
    ]);
    const prepended = initial.update({
      changes: { from: 0, insert: "前置き " },
    }).state;
    expect(decorationSummary(prepended)).toEqual([
      { from: 4, to: 6, kind: "replace" },
      { from: 6, to: 8, kind: "lp-strong" },
      { from: 8, to: 10, kind: "replace" },
    ]);
  });

  it("見出しは行装飾になり、カーソルのある行ではmarkerを開示する", () => {
    const doc = "== 見出し\n本文です。";
    const heading: NoteSourceSpan = {
      kind: "heading",
      span: byteSpan(0, 13),
      content_span: byteSpan(3, 12),
      marker_spans: [byteSpan(0, 2), byteSpan(2, 3)],
      level: 1,
    };
    const state = stateWithSpans(doc, heading ? [heading] : []);
    const away = state.update({
      selection: EditorSelection.cursor(doc.length),
    }).state;
    expect(decorationSummary(away)).toEqual([
      { from: 0, to: 0, kind: "lp-heading lp-heading-1" },
      { from: 0, to: 2, kind: "replace" },
      { from: 2, to: 3, kind: "replace" },
    ]);
    const onLine = away.update({
      selection: EditorSelection.cursor(4),
    }).state;
    expect(decorationSummary(onLine)).toEqual([
      { from: 0, to: 0, kind: "lp-heading lp-heading-1" },
    ]);
  });

  it("インライン数式はカーソル外でwidgetになり、交差で原文を開示する", () => {
    // "面積は stem:[\pi r^2] です。" のstem部分。バイト位置はUTF-8で数える。
    const doc = "面積は stem:[\\pi r^2] です。";
    const inlineMath: NoteSourceSpan = {
      kind: "inline_math",
      span: byteSpan(10, 24),
      content_span: byteSpan(16, 23),
      marker_spans: [byteSpan(10, 16), byteSpan(23, 24)],
    };
    const state = stateWithSpans(doc, [inlineMath]);
    const away = state.update({
      selection: EditorSelection.cursor(0),
    }).state;
    const summary = decorationSummary(away);
    expect(summary).toHaveLength(1);
    expect(summary[0].kind).toBe("widget");
    const revealed = away.update({
      selection: EditorSelection.cursor(8),
    }).state;
    expect(decorationSummary(revealed)).toEqual([
      { from: 4, to: 18, kind: "lp-math-source" },
    ]);
  });

  it("ブロック数式は原文の直後へ組版widgetを併記する", () => {
    const doc = "[latexmath]\n++++\nE = mc^2\n++++\n";
    const mathBlock: NoteSourceSpan = {
      kind: "math_block",
      span: byteSpan(0, 31),
      content_span: byteSpan(17, 26),
      marker_spans: [byteSpan(0, 12), byteSpan(12, 17)],
    };
    const state = stateWithSpans(doc, [mathBlock]);
    const summary = decorationSummary(state);
    expect(summary).toHaveLength(1);
    expect(summary[0].kind).toBe("widget");
    expect(summary[0].from).toBe(doc.length);
  });

  it("引用ブロックの各行を行装飾し、区切り行を淡色にする", () => {
    const doc = "[quote]\n____\n引用文です。\n____\n";
    const quote: NoteSourceSpan = {
      kind: "quote",
      span: byteSpan(0, 37),
      content_span: byteSpan(13, 31),
      marker_spans: [byteSpan(0, 7), byteSpan(8, 12), byteSpan(32, 36)],
    };
    const state = stateWithSpans(doc, [quote]);
    expect(decorationSummary(state)).toEqual([
      { from: 0, to: 0, kind: "lp-block-quote lp-block-meta" },
      { from: 8, to: 8, kind: "lp-block-quote lp-block-meta" },
      { from: 13, to: 13, kind: "lp-block-quote" },
      { from: 20, to: 20, kind: "lp-block-quote lp-block-meta" },
    ]);
  });

  it("admonitionのラベルを強調し、行装飾を掛ける", () => {
    const doc = "NOTE: 注意です。\n";
    const admonition: NoteSourceSpan = {
      kind: "admonition",
      span: byteSpan(0, 22),
      content_span: byteSpan(6, 21),
      marker_spans: [byteSpan(0, 5)],
    };
    const state = stateWithSpans(doc, [admonition]);
    expect(decorationSummary(state)).toEqual([
      { from: 0, to: 0, kind: "lp-block-admonition" },
      { from: 0, to: 5, kind: "lp-block-label" },
    ]);
  });

  it("リスト項目のmarkerをビュレットへ置き換え、カーソルの行では開示する", () => {
    const doc = "* 項目1\n** 項目2\n";
    const items: NoteSourceSpan[] = [
      {
        kind: "list_item",
        span: byteSpan(0, 9),
        content_span: byteSpan(2, 9),
        marker_spans: [byteSpan(0, 1), byteSpan(1, 2)],
      },
      {
        kind: "list_item",
        span: byteSpan(10, 20),
        content_span: byteSpan(13, 20),
        marker_spans: [byteSpan(10, 12), byteSpan(12, 13)],
      },
    ];
    // カーソルが末尾(空の3行目)にある間は両方のmarkerが置換される。
    const state = stateWithSpans(doc, items);
    expect(decorationSummary(state)).toEqual([
      { from: 0, to: 1, kind: "widget" },
      { from: 6, to: 8, kind: "widget" },
    ]);
    // 2行目へカーソルを移すと、その行のmarkerだけが開示される。
    const revealed = state.update({
      selection: EditorSelection.cursor(9),
    }).state;
    expect(decorationSummary(revealed)).toEqual([
      { from: 0, to: 1, kind: "widget" },
      { from: 6, to: 8, kind: "lp-list-marker lp-list-marker-2" },
    ]);
  });

  it("文書属性の行を淡色の行装飾にする", () => {
    const doc = "= 題名\n:marginalis-tags: rust\n";
    const attribute: NoteSourceSpan = {
      kind: "document_attribute",
      span: byteSpan(9, 30),
    };
    const state = stateWithSpans(doc, [attribute]);
    expect(decorationSummary(state)).toEqual([
      { from: 5, to: 5, kind: "lp-doc-attribute" },
    ]);
  });

  it("IME変換中の入力では装飾を組み立て直さない", () => {
    const initial = stateWithSpans(source, [strongSpan]);
    const composing = initial.update({
      changes: { from: source.length, insert: "か" },
      selection: EditorSelection.cursor(source.length + 1),
      userEvent: "input.type.compose",
    }).state;
    // 折り畳みとmarkは変換前の組み立てのまま残る。
    expect(decorationSummary(composing)).toEqual([
      { from: 0, to: 2, kind: "replace" },
      { from: 2, to: 4, kind: "lp-strong" },
      { from: 4, to: 6, kind: "replace" },
    ]);
  });

  it("IME変換中も変更より後ろの装飾位置を本文へ追従させる", () => {
    const doc = `前文\n\n${source}`;
    const prefixBytes = new TextEncoder().encode("前文\n\n").length;
    const shiftedStrongSpan: NoteSourceSpan = {
      ...strongSpan,
      span: byteSpan(
        prefixBytes + strongSpan.span.start,
        prefixBytes + strongSpan.span.end,
      ),
      content_span: byteSpan(
        prefixBytes + (strongSpan.content_span?.start ?? 0),
        prefixBytes + (strongSpan.content_span?.end ?? 0),
      ),
      marker_spans: (strongSpan.marker_spans ?? []).map((marker) =>
        byteSpan(prefixBytes + marker.start, prefixBytes + marker.end),
      ),
    };
    const initial = stateWithSpans(doc, [shiftedStrongSpan]);
    const composing = initial.update({
      changes: { from: 0, insert: "か" },
      selection: EditorSelection.cursor(1),
      userEvent: "input.type.compose",
    }).state;

    expect(decorationSummary(composing)).toEqual([
      { from: 5, to: 7, kind: "replace" },
      { from: 7, to: 9, kind: "lp-strong" },
      { from: 9, to: 11, kind: "replace" },
    ]);
    expect(composing.doc.sliceString(5, 11)).toBe("**太字**");
  });
});
