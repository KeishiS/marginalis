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
});
