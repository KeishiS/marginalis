import { EditorState, Facet, Range } from "@codemirror/state";
import { Decoration, DecorationSet } from "@codemirror/view";

import { MathMacro, NoteSourceSpanKind } from "../../api";
import { LiveSpan } from "./spans";
import { ListBulletWidget, MathWidget } from "./widgets";

/** MathJaxの実行時styleへ付けるCSP nonce。livePreview拡張の組み込み時に与える。 */
export const livePreviewStyleNonce = Facet.define<string, string>({
  combine: (values) => values[0] ?? "",
});

/** mark装飾を掛けるインライン記法と、そのCSS class。 */
const INLINE_MARK_CLASSES: Partial<Record<NoteSourceSpanKind, string>> = {
  strong: "lp-strong",
  emphasis: "lp-emphasis",
  highlight: "lp-highlight",
  subscript: "lp-subscript",
  superscript: "lp-superscript",
  monospace: "lp-monospace",
  link: "lp-link",
  cross_reference: "lp-reference",
  citation: "lp-citation",
};

const HEADING_KINDS: ReadonlySet<NoteSourceSpanKind> = new Set([
  "document_title",
  "heading",
]);

/** 行装飾を掛けるブロック記法と、そのCSS class。 */
const BLOCK_LINE_CLASSES: Partial<Record<NoteSourceSpanKind, string>> = {
  quote: "lp-block-quote",
  admonition: "lp-block-admonition",
  example: "lp-block-example",
  literal_block: "lp-block-literal",
  source_block: "lp-block-source",
  table: "lp-block-table",
};

/**
 * span注釈と選択範囲から装飾を組み立てる。
 *
 * 選択範囲が記法全体の範囲と交差(端の接触を含む)しているspanは「開示中」として扱い、
 * 記法文字の折り畳みを出さない。mark装飾と見出しの行装飾は開示中も維持し、
 * 表示の連続性を保つ。
 */
export function buildDecorations(
  state: EditorState,
  spans: readonly LiveSpan[],
  mathMacros: readonly MathMacro[],
): DecorationSet {
  const docLength = state.doc.length;
  const styleNonce = state.facet(livePreviewStyleNonce);
  const macros = [...mathMacros];
  const macroSignature = JSON.stringify(mathMacros);
  const decorations: Range<Decoration>[] = [];
  for (const span of spans) {
    if (span.to > docLength || span.from >= span.to) continue;
    const revealed = intersectsSelection(state, span);
    if (HEADING_KINDS.has(span.kind)) {
      const line = state.doc.lineAt(span.from);
      const level = span.kind === "document_title" ? 0 : (span.level ?? 1);
      decorations.push(
        Decoration.line({
          class: `lp-heading lp-heading-${Math.min(level, 5)}`,
        }).range(line.from),
      );
      if (!revealed) {
        pushMarkerFolds(decorations, span, docLength);
      }
      continue;
    }
    if (span.kind === "inline_math") {
      const latex = formulaSource(state, span);
      if (revealed || latex === null) {
        decorations.push(
          Decoration.mark({ class: "lp-math-source" }).range(
            span.from,
            span.to,
          ),
        );
      } else {
        decorations.push(
          Decoration.replace({
            widget: new MathWidget(
              latex,
              false,
              macros,
              macroSignature,
              styleNonce,
            ),
          }).range(span.from, span.to),
        );
      }
      continue;
    }
    if (span.kind === "math_block") {
      const latex = formulaSource(state, span);
      if (latex !== null) {
        // 原文の行は残したまま、ブロックの直後へ組版結果を併記する。ブロック末尾の
        // 改行の有無で終端が行頭にも行末にもなるため、行境界に合わせて位置を決める。
        const anchor = Math.min(span.to, docLength);
        const line = state.doc.lineAt(anchor);
        const atLineStart = anchor === line.from;
        decorations.push(
          Decoration.widget({
            widget: new MathWidget(
              latex,
              true,
              macros,
              macroSignature,
              styleNonce,
            ),
            block: true,
            side: atLineStart ? -1 : 1,
          }).range(atLineStart ? anchor : line.to),
        );
      }
      continue;
    }
    const blockClass = BLOCK_LINE_CLASSES[span.kind];
    if (blockClass !== undefined) {
      pushBlockLines(decorations, state, span, blockClass);
      continue;
    }
    if (span.kind === "list_item") {
      pushListItem(decorations, state, span);
      continue;
    }
    if (span.kind === "document_attribute") {
      decorations.push(
        Decoration.line({ class: "lp-doc-attribute" }).range(
          state.doc.lineAt(span.from).from,
        ),
      );
      continue;
    }
    if (span.kind === "anchor") {
      decorations.push(
        Decoration.mark({ class: "lp-anchor" }).range(span.from, span.to),
      );
      continue;
    }
    const markClass = INLINE_MARK_CLASSES[span.kind];
    if (markClass === undefined) continue;
    if (span.contentFrom < span.contentTo && span.contentTo <= docLength) {
      decorations.push(
        Decoration.mark({ class: markClass }).range(
          span.contentFrom,
          span.contentTo,
        ),
      );
    }
    if (!revealed) {
      pushMarkerFolds(decorations, span, docLength);
    }
  }
  return Decoration.set(decorations, true);
}

/**
 * ブロック記法の各行へ行装飾を掛ける。
 *
 * 属性行と区切り行(marker)は内容と見分けられるよう淡色のclassも重ねる。行装飾だけで
 * 折り畳みは行わないため、カーソル開示の対象にしない。
 */
function pushBlockLines(
  decorations: Range<Decoration>[],
  state: EditorState,
  span: LiveSpan,
  blockClass: string,
) {
  const lastPosition = Math.min(span.to, state.doc.length);
  // 行全体を占めるmarker(属性行や区切り行)は行ごと淡色にし、行の一部のmarker
  // (admonitionの`NOTE:`ラベルなど)はその範囲だけをラベルとして強調する。
  const metaLineStarts = new Set<number>();
  for (const marker of span.markers) {
    if (marker.to > state.doc.length) continue;
    const line = state.doc.lineAt(marker.from);
    if (marker.from <= line.from && marker.to >= line.to) {
      metaLineStarts.add(line.from);
    } else if (marker.from < marker.to) {
      decorations.push(
        Decoration.mark({ class: "lp-block-label" }).range(
          marker.from,
          marker.to,
        ),
      );
    }
  }
  let line = state.doc.lineAt(span.from);
  for (;;) {
    // ブロック終端が改行の直後を指す場合、その行はブロックに含めない。
    if (line.from >= lastPosition) break;
    const meta = metaLineStarts.has(line.from) ? " lp-block-meta" : "";
    decorations.push(
      Decoration.line({ class: `${blockClass}${meta}` }).range(line.from),
    );
    if (line.to >= lastPosition) break;
    line = state.doc.lineAt(line.to + 1);
  }
}

/**
 * リスト項目の行頭markerをビュレット表示へ置き換える。
 *
 * markerの深さは記法文字の長さ(`*`の繰り返し数)から決め、カーソルがmarkerの行に
 * ある間は原文を開示する。
 */
function pushListItem(
  decorations: Range<Decoration>[],
  state: EditorState,
  span: LiveSpan,
) {
  const marker = span.markers[0];
  if (!marker || marker.to > state.doc.length) return;
  const markerLine = state.doc.lineAt(marker.from);
  const revealed = state.selection.ranges.some(
    (range) => range.from <= markerLine.to && range.to >= markerLine.from,
  );
  const depth = Math.min(marker.to - marker.from, 3);
  if (revealed) {
    decorations.push(
      Decoration.mark({
        class: `lp-list-marker lp-list-marker-${depth}`,
      }).range(marker.from, marker.to),
    );
  } else {
    decorations.push(
      Decoration.replace({
        widget: new ListBulletWidget(depth),
      }).range(marker.from, marker.to),
    );
  }
}

/** 数式の本文部分を原文から読む。範囲が壊れている場合は`null`。 */
function formulaSource(state: EditorState, span: LiveSpan): string | null {
  if (span.contentFrom >= span.contentTo || span.contentTo > state.doc.length) {
    return null;
  }
  const value = state.doc.sliceString(span.contentFrom, span.contentTo).trim();
  return value.length > 0 ? value : null;
}

function pushMarkerFolds(
  decorations: Range<Decoration>[],
  span: LiveSpan,
  docLength: number,
) {
  for (const marker of span.markers) {
    if (marker.from >= marker.to || marker.to > docLength) continue;
    decorations.push(Decoration.replace({}).range(marker.from, marker.to));
  }
}

function intersectsSelection(state: EditorState, span: LiveSpan): boolean {
  return state.selection.ranges.some(
    (range) => range.from <= span.to && range.to >= span.from,
  );
}
