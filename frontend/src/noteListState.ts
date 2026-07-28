import { NoteListEntry } from "./api";

export const NOTE_LIST_PAGE_SIZE = 20;

export interface NoteListQuery {
  tags: string[];
  updatedAfter: string;
  page: number;
}

export interface NoteListPage {
  notes: NoteListEntry[];
  page: number;
  pageCount: number;
  total: number;
}

export function parseNoteListQuery(search: string): NoteListQuery {
  const parameters = new URLSearchParams(search);
  const tags = parameters
    .getAll("tag")
    .flatMap((value) => value.split(","))
    .map((value) => value.trim())
    .filter((value, index, values) => value && values.indexOf(value) === index);
  const updatedAfter = validDate(parameters.get("updated_after") ?? "");
  const requestedPage = Number(parameters.get("page") ?? "1");
  return {
    tags,
    updatedAfter,
    page:
      Number.isSafeInteger(requestedPage) && requestedPage > 0
        ? requestedPage
        : 1,
  };
}

export function noteListSearch(
  query: NoteListQuery,
  page = query.page,
): string {
  const parameters = new URLSearchParams();
  for (const tag of query.tags) parameters.append("tag", tag);
  if (query.updatedAfter) {
    parameters.set("updated_after", query.updatedAfter);
  }
  if (page > 1) parameters.set("page", String(page));
  const value = parameters.toString();
  return value ? `?${value}` : "";
}

export function selectNoteListPage(
  notes: NoteListEntry[],
  query: NoteListQuery,
): NoteListPage {
  const cutoff = query.updatedAfter
    ? Date.parse(`${query.updatedAfter}T00:00:00Z`)
    : null;
  const filtered = notes.filter(
    (note) =>
      query.tags.every((tag) => note.tags.includes(tag)) &&
      (cutoff === null || note.updated_at_ms >= cutoff),
  );
  const pageCount = Math.max(
    1,
    Math.ceil(filtered.length / NOTE_LIST_PAGE_SIZE),
  );
  const page = Math.min(query.page, pageCount);
  const start = (page - 1) * NOTE_LIST_PAGE_SIZE;
  return {
    notes: filtered.slice(start, start + NOTE_LIST_PAGE_SIZE),
    page,
    pageCount,
    total: filtered.length,
  };
}

function validDate(value: string): string {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(value)) return "";
  const date = new Date(`${value}T00:00:00Z`);
  return Number.isNaN(date.getTime()) ||
    date.toISOString().slice(0, 10) !== value
    ? ""
    : value;
}
