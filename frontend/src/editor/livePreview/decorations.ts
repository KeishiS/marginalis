import { EditorState, Range } from "@codemirror/state";
import { Decoration, DecorationSet } from "@codemirror/view";

import { NoteSourceSpanKind } from "../../api";
import { LiveSpan } from "./spans";

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
): DecorationSet {
  const docLength = state.doc.length;
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
