import { NoteSourceSpan, NoteSourceSpanKind, Utf8ByteSpan } from "../../api";
import { utf8ByteOffsetsToTextOffsets } from "../../textPosition";

/** UTF-16オフセットへ変換済みの、装飾1件分のspan注釈。 */
export interface LiveSpan {
  kind: NoteSourceSpanKind;
  /** 記法全体の範囲。カーソル開示の判定に使う。 */
  from: number;
  to: number;
  /** 記法文字を除いた本文部分。mark装飾を掛ける範囲。 */
  contentFrom: number;
  contentTo: number;
  /** カーソルが離れているときに折り畳む記法文字の範囲。 */
  markers: { from: number; to: number }[];
  /** 見出しの深さ。`==`が1。 */
  level: number | null;
}

/**
 * 契約のspan注釈を、解析対象と同じ本文を使ってUTF-16オフセットの内部表現へ直す。
 *
 * すべてのバイトオフセットを集めて1回の走査で変換する。範囲が逆転しているものと
 * 本文の外を指すものは、装飾を誤った位置へ掛けないよう捨てる。
 */
export function toLiveSpans(
  source: string,
  spans: readonly NoteSourceSpan[],
): LiveSpan[] {
  const byteOffsets: number[] = [];
  const collect = (span: Utf8ByteSpan) => {
    byteOffsets.push(span.start, span.end);
  };
  for (const span of spans) {
    collect(span.span);
    if (span.content_span) collect(span.content_span);
    for (const marker of span.marker_spans ?? []) collect(marker);
  }
  const order = byteOffsets
    .map((offset, index) => ({ offset, index }))
    .sort((a, b) => a.offset - b.offset);
  const sortedTextOffsets = utf8ByteOffsetsToTextOffsets(
    source,
    order.map((item) => item.offset),
  );
  const textOffsets = new Array<number>(byteOffsets.length);
  order.forEach((item, position) => {
    textOffsets[item.index] = sortedTextOffsets[position];
  });

  const result: LiveSpan[] = [];
  let cursor = 0;
  const take = () => textOffsets[cursor++];
  for (const span of spans) {
    const from = take();
    const to = take();
    const content = span.content_span
      ? { from: take(), to: take() }
      : { from, to };
    const markers = (span.marker_spans ?? []).map(() => {
      const markerFrom = take();
      const markerTo = take();
      return { from: markerFrom, to: markerTo };
    });
    if (from >= to || to > source.length) continue;
    if (content.from > content.to) continue;
    result.push({
      kind: span.kind,
      from,
      to,
      contentFrom: content.from,
      contentTo: content.to,
      markers: markers.filter(
        (marker) => marker.from < marker.to && marker.to <= source.length,
      ),
      level: span.level ?? null,
    });
  }
  return result;
}
