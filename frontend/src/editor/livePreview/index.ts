import { Extension } from "@codemirror/state";

import { livePreviewField, setLiveSpans } from "./state";
import { LiveSpan, toLiveSpans } from "./spans";

export { livePreviewField, setLiveSpans, toLiveSpans };
export type { LiveSpan };

/**
 * 編集欄のLive Preview拡張。
 *
 * span注釈(ADR 0016)から装飾を導き、選択範囲が交差した記法はソースを開示する。
 * 装飾は表示だけを変え、本文の文字列には影響しない。
 */
export function livePreview(): Extension {
  return [livePreviewField];
}
