import {
  Completion,
  CompletionContext,
  CompletionResult,
} from "@codemirror/autocomplete";

import { listNotes, searchBibliography } from "../api";

/** 補完sourceの型。対象外の文脈ではnullを返して他の補完を妨げない。 */
export type EditorCompletionSource = (
  context: CompletionContext,
) => Promise<CompletionResult | null>;

/**
 * 候補の遅延取得と使い回し。
 *
 * 専用のendpointを設けず、既存の認可済みAPIから候補を導出する(個人利用の
 * 規模では一覧の取得で十分で、認可も既存APIと同じ範囲になる)。取得は最初の
 * 補完要求まで遅延し、以後は編集セッション中の値を使い回す。失敗は空の候補と
 * して扱い、次の要求で取得し直す(候補が出ないだけで編集は続けられるため)。
 */
function cachedCandidateLoader<T>(
  load: () => Promise<T[]>,
): () => Promise<T[]> {
  let cached: Promise<T[]> | null = null;
  return () => {
    cached ??= load().catch(() => {
      cached = null;
      return [];
    });
    return cached;
  };
}

export function createTagCandidateLoader(
  apiBase: string,
): () => Promise<string[]> {
  return cachedCandidateLoader(() =>
    listNotes(apiBase).then((notes) =>
      [...new Set(notes.flatMap((note) => note.tags))].sort((left, right) =>
        left.localeCompare(right, "ja"),
      ),
    ),
  );
}

export interface NoteCandidate {
  noteId: string;
  title: string;
}

export function createNoteCandidateLoader(
  apiBase: string,
): () => Promise<NoteCandidate[]> {
  return cachedCandidateLoader(() =>
    listNotes(apiBase).then((notes) =>
      notes
        .map((note) => ({ noteId: note.note_id, title: note.title }))
        .sort((left, right) => left.title.localeCompare(right.title, "ja")),
    ),
  );
}

export function createCitationKeyLoader(
  apiBase: string,
): () => Promise<string[]> {
  return cachedCandidateLoader(() =>
    searchBibliography(apiBase).then((items) =>
      items.map((item) => item.citation_key).sort(),
    ),
  );
}

const TAGS_ATTRIBUTE = ":marginalis-tags:";

/**
 * タグ属性行でだけ働く補完source。
 *
 * カンマ区切りの現在の区画を補完対象とし、同じ行に入力済みのタグは候補から
 * 除く。空の区画では明示的な要求(Ctrl+Space)のときだけ候補を出す。
 */
export function tagCompletionSource(
  loadTags: () => Promise<string[]>,
): EditorCompletionSource {
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

/**
 * ノート間参照の補完source。
 *
 * `xref:note:`に続けて題名の一部を入力すると、閲覧できるノートの題名で
 * 絞り込み、確定で`<note ID>[]`を挿入する。表示文を空にすると閲覧画面が
 * 参照先の題名を表示するため、挿入形はそれに合わせる。
 */
export function noteReferenceCompletionSource(
  loadNotes: () => Promise<NoteCandidate[]>,
): EditorCompletionSource {
  return async (context) => {
    const match = context.matchBefore(/xref:note:[^\s[\]]*/);
    if (!match) return null;
    const from = match.from + "xref:note:".length;
    const options: Completion[] = (await loadNotes()).map((note) => ({
      label: note.title,
      type: "text",
      apply: `${note.noteId}[]`,
    }));
    if (options.length === 0) return null;
    return { from, options, validFor: /^[^\s[\]]*$/ };
  };
}

/**
 * 引用の補完source。
 *
 * `cite:[`の中でcitation keyを補完する。カンマ区切りの現在の区画を対象とし、
 * 同じ括弧内に入力済みのkeyは候補から除く。
 */
export function citationKeyCompletionSource(
  loadKeys: () => Promise<string[]>,
): EditorCompletionSource {
  return async (context) => {
    const match = context.matchBefore(/cite:\[[^\]]*/);
    if (!match) return null;
    const value = context.state.sliceDoc(
      match.from + "cite:[".length,
      context.pos,
    );
    const segmentOffset = value.lastIndexOf(",") + 1;
    const segment = value.slice(segmentOffset);
    const typed = segment.trimStart();
    const from =
      match.from +
      "cite:[".length +
      segmentOffset +
      (segment.length - typed.length);
    const entered = new Set(
      value
        .split(",")
        .map((key) => key.trim())
        .filter(Boolean),
    );
    entered.delete(typed);
    const options: Completion[] = (await loadKeys())
      .filter((key) => !entered.has(key))
      .map((key) => ({ label: key, type: "constant" }));
    if (options.length === 0) return null;
    return { from, options, validFor: /^[^,\]]*$/ };
  };
}
