import { StateEffect, StateField, Transaction } from "@codemirror/state";
import { Decoration, DecorationSet, EditorView } from "@codemirror/view";

import { buildDecorations } from "./decorations";
import { LiveSpan } from "./spans";

/** サーバー解析の結果で、保持しているspan注釈を丸ごと置き換える。 */
export const setLiveSpans = StateEffect.define<readonly LiveSpan[]>();

interface LivePreviewValue {
  spans: readonly LiveSpan[];
  decorations: DecorationSet;
}

/**
 * span注釈と、そこから導いた装飾を保持する。
 *
 * 本文の変更では既存spanの位置を変換して追従させ、次の解析結果で置き換える。
 * 選択範囲の変化ではカーソル開示の状態だけが変わるため装飾を組み立て直す。
 * IME変換中の入力では組み立てを保留し、変換の確定後の更新で追い付く。
 */
export const livePreviewField = StateField.define<LivePreviewValue>({
  create() {
    return { spans: [], decorations: Decoration.none };
  },
  update(value, transaction) {
    let spans = value.spans;
    let changed = false;
    if (transaction.docChanged) {
      spans = mapSpans(spans, transaction);
      changed = true;
    }
    for (const effect of transaction.effects) {
      if (effect.is(setLiveSpans)) {
        spans = effect.value;
        changed = true;
      }
    }
    if (!changed && !transaction.selection) {
      return value;
    }
    if (isComposingInput(transaction)) {
      return { spans, decorations: value.decorations };
    }
    return {
      spans,
      decorations: buildDecorations(transaction.state, spans),
    };
  },
  provide: (field) =>
    EditorView.decorations.from(field, (value) => value.decorations),
});

function mapSpans(
  spans: readonly LiveSpan[],
  transaction: Transaction,
): LiveSpan[] {
  const result: LiveSpan[] = [];
  for (const span of spans) {
    const from = transaction.changes.mapPos(span.from, 1);
    const to = transaction.changes.mapPos(span.to, -1);
    if (from >= to) continue;
    const contentFrom = transaction.changes.mapPos(span.contentFrom, 1);
    const contentTo = transaction.changes.mapPos(span.contentTo, -1);
    result.push({
      ...span,
      from,
      to,
      contentFrom: Math.min(contentFrom, contentTo),
      contentTo: Math.max(contentFrom, contentTo),
      markers: span.markers
        .map((marker) => ({
          from: transaction.changes.mapPos(marker.from, 1),
          to: transaction.changes.mapPos(marker.to, -1),
        }))
        .filter((marker) => marker.from < marker.to),
    });
  }
  return result;
}

function isComposingInput(transaction: Transaction): boolean {
  return transaction.isUserEvent("input.type.compose");
}
