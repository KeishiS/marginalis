import {
  Completion,
  CompletionContext,
  CompletionResult,
} from "@codemirror/autocomplete";

import { listNotes } from "../api";

/**
 * `:marginalis-tags:`属性行のタグ補完。
 *
 * 候補は閲覧できるノートで使用中のタグから作る。専用のendpointを設けず、
 * ノート一覧APIの結果から導出する(個人利用の規模では一覧の取得で十分で、
 * 認可も一覧APIと同じ範囲になる)。取得は最初の補完要求まで遅延し、以後は
 * 編集セッション中の値を使い回す。
 */
export function createTagCandidateLoader(
  apiBase: string,
): () => Promise<string[]> {
  let cached: Promise<string[]> | null = null;
  return () => {
    cached ??= listNotes(apiBase).then(
      (notes) =>
        [...new Set(notes.flatMap((note) => note.tags))].sort((left, right) =>
          left.localeCompare(right, "ja"),
        ),
      () => {
        // 候補が出ないだけで編集は続けられるため、失敗は空の候補として扱い、
        // 次の補完要求で取得し直す。
        cached = null;
        return [];
      },
    );
    return cached;
  };
}

const TAGS_ATTRIBUTE = ":marginalis-tags:";

/**
 * タグ属性行でだけ働くCodeMirrorの補完source。
 *
 * カンマ区切りの現在の区画を補完対象とし、同じ行に入力済みのタグは候補から
 * 除く。本文の行では何も返さず、他の補完を妨げない。
 */
export function tagCompletionSource(
  loadTags: () => Promise<string[]>,
): (context: CompletionContext) => Promise<CompletionResult | null> {
  return async (context) => {
    const line = context.state.doc.lineAt(context.pos);
    if (!line.text.startsWith(TAGS_ATTRIBUTE)) return null;
    const valueStart = line.from + TAGS_ATTRIBUTE.length;
    if (context.pos < valueStart) return null;
    const value = context.state.sliceDoc(valueStart, context.pos);
    const segmentOffset = value.lastIndexOf(",") + 1;
    const segment = value.slice(segmentOffset);
    const typed = segment.trimStart();
    const from = valueStart + segmentOffset + (segment.length - typed.length);
    if (typed === "" && !context.explicit) return null;
    const entered = new Set(
      line.text
        .slice(TAGS_ATTRIBUTE.length)
        .split(",")
        .map((tag) => tag.trim())
        .filter(Boolean),
    );
    entered.delete(typed);
    const options: Completion[] = (await loadTags())
      .filter((tag) => !entered.has(tag))
      .map((tag) => ({ label: tag, type: "keyword" }));
    if (options.length === 0) return null;
    return { from, options, validFor: /^[^,]*$/ };
  };
}
