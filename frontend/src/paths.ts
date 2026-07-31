import { noteListSearch, parseNoteListQuery } from "./noteListState";

/** 画面URLの組み立てに必要な設定。 */
export interface PathContext {
  basePath: string;
  search: string;
}

export function externalPath(basePath: string, path: string): string {
  const base = basePath === "/" ? "" : basePath.replace(/\/$/, "");
  return `${base}${path}`;
}

/**
 * 一覧の絞り込み条件を正規化した問い合わせ文字列へ直す。
 *
 * 画面をまたいで同じ条件が同じURLになるよう、生の値をそのまま連結しない。
 */
export function canonicalSearch(search: string): string {
  return noteListSearch(parseNoteListQuery(search));
}

/** 一覧画面のURL。ページを指定すると、その位置を含める。 */
export function listPath(context: PathContext, page?: number): string {
  const query = parseNoteListQuery(context.search);
  return externalPath(context.basePath, `/${noteListSearch(query, page)}`);
}

/** ノートの閲覧画面のURL。 */
export function notePath(context: PathContext, noteId: string): string {
  return withSearch(context, `/notes/${noteId}`);
}

/** ノートの編集画面のURL。 */
export function editPath(context: PathContext, noteId: string): string {
  return withSearch(context, `/notes/${noteId}/edit`);
}

/** ノートの共有設定画面のURL。 */
export function accessPath(context: PathContext, noteId: string): string {
  return withSearch(context, `/notes/${noteId}/access`);
}

function withSearch(context: PathContext, path: string): string {
  return `${externalPath(context.basePath, path)}${canonicalSearch(context.search)}`;
}
