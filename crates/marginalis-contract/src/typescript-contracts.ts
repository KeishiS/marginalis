// このファイルはmarginalis-contractが生成します。直接編集しないでください。
export interface Note {
  note_id: string;
  title: string;
  source: string;
  tags: string[];
  created_at_ms: number;
  updated_at_ms: number;
  revision: number;
}
export interface NoteSummary {
  note_id: string;
  title: string;
  tags: string[];
  updated_at_ms: number;
  revision: number;
}
export type NoteAccess = "read" | "edit" | "manage";
export interface NoteListEntry extends NoteSummary {
  access: NoteAccess;
}
export interface RelatedNotes {
  outgoing: NoteSummary[];
  incoming: NoteSummary[];
}
export interface NoteView {
  note: Note;
  access: NoteAccess;
  html: string;
  related: RelatedNotes;
}
export interface NoteDraft {
  source: string;
}
export interface NotePreview {
  html: string;
  diagnostics: ValidationDiagnostic[];
}
export type NotePermission = "read" | "edit";
export interface NoteAclEntry {
  subject: string;
  permission: NotePermission;
}
export interface NoteAclGrant extends NoteAclEntry {
  issuer: string;
}
export interface ValidationDiagnostic {
  code: string;
  severity: "error" | "warning" | "information" | "hint";
  target: ValidationTarget;
  span?: { start: number; end: number; unit: "utf8_byte" };
  message: string;
}
export type ValidationTarget =
  | { field: "source" | "title" | "body" | "tags" }
  | { field: "tag" | "acl_entry"; index: number };
export type ProblemCode =
  | "authentication_required"
  | "authentication_unavailable"
  | "csrf_rejected"
  | "csrf_required"
  | "csrf_invalid"
  | "same_origin_required"
  | "origin_not_allowed"
  | "not_found"
  | "forbidden"
  | "conflict"
  | "precondition_required"
  | "invalid_request"
  | "validation_failed"
  | "render_failed"
  | "unavailable";
export interface Problem {
  code: ProblemCode | "invalid_response" | "network_error";
  message: string;
  diagnostics?: ValidationDiagnostic[];
}

export class ApiError extends Error {
  constructor(
    readonly status: number,
    readonly problem: Problem,
  ) {
    super(problem.message);
  }
}

export function parseNote(value: unknown): Note {
  const object = record(value, "note");
  return {
    note_id: text(object.note_id, "note.note_id"),
    title: text(object.title, "note.title"),
    source: text(object.source, "note.source"),
    tags: textArray(object.tags, "note.tags"),
    created_at_ms: integer(object.created_at_ms, "note.created_at_ms"),
    updated_at_ms: integer(object.updated_at_ms, "note.updated_at_ms"),
    revision: positiveInteger(object.revision, "note.revision"),
  };
}

export function parseNotePreview(value: unknown): NotePreview {
  const object = record(value, "preview");
  return {
    html: text(object.html, "preview.html"),
    diagnostics: parseValidationDiagnostics(
      object.diagnostics,
      "preview.diagnostics",
    ),
  };
}

export function parseNoteSummary(value: unknown): NoteSummary {
  const object = record(value, "note summary");
  return {
    note_id: text(object.note_id, "note summary.note_id"),
    title: text(object.title, "note summary.title"),
    tags: textArray(object.tags, "note summary.tags"),
    updated_at_ms: integer(object.updated_at_ms, "note summary.updated_at_ms"),
    revision: positiveInteger(object.revision, "note summary.revision"),
  };
}

export function parseNoteSummaries(value: unknown): NoteSummary[] {
  if (!Array.isArray(value)) throw new Error("note summaries are invalid");
  return value.map(parseNoteSummary);
}

export function parseNoteListEntry(value: unknown): NoteListEntry {
  const summary = parseNoteSummary(value);
  const object = record(value, "note list entry");
  const access = object.access;
  if (access !== "read" && access !== "edit" && access !== "manage") {
    throw new Error("note list entry.access is invalid");
  }
  return { ...summary, access };
}

export function parseNoteListEntries(value: unknown): NoteListEntry[] {
  if (!Array.isArray(value)) throw new Error("note list entries are invalid");
  return value.map(parseNoteListEntry);
}

export function parseNoteView(value: unknown): NoteView {
  const object = record(value, "note view");
  const access = object.access;
  if (access !== "read" && access !== "edit" && access !== "manage") {
    throw new Error("note view.access is invalid");
  }
  const related = record(object.related, "note view.related");
  return {
    note: parseNote(object.note),
    access,
    html: text(object.html, "note view.html"),
    related: {
      outgoing: parseNoteSummaries(related.outgoing),
      incoming: parseNoteSummaries(related.incoming),
    },
  };
}

export function parseNoteAcl(value: unknown): { entries: NoteAclGrant[] } {
  const object = record(value, "acl");
  if (!Array.isArray(object.entries)) {
    throw new Error("acl.entries is invalid");
  }
  return {
    entries: object.entries.map((entry, index) => {
      const grant = record(entry, `acl.entries[${index}]`);
      const permission = grant.permission;
      if (permission !== "read" && permission !== "edit") {
        throw new Error(`acl.entries[${index}].permission is invalid`);
      }
      return {
        issuer: text(grant.issuer, `acl.entries[${index}].issuer`),
        subject: text(grant.subject, `acl.entries[${index}].subject`),
        permission,
      };
    }),
  };
}

export function parseProblem(value: unknown): Problem {
  const object = record(value, "problem");
  const code = problemCode(object.code);
  return {
    code,
    message: text(object.message, "problem.message"),
    diagnostics: Array.isArray(object.diagnostics)
      ? parseValidationDiagnostics(object.diagnostics, "problem.diagnostics")
      : undefined,
  };
}

function problemCode(value: unknown): ProblemCode {
  if (
    value !== "authentication_required" &&
    value !== "authentication_unavailable" &&
    value !== "csrf_rejected" &&
    value !== "csrf_required" &&
    value !== "csrf_invalid" &&
    value !== "same_origin_required" &&
    value !== "origin_not_allowed" &&
    value !== "not_found" &&
    value !== "forbidden" &&
    value !== "conflict" &&
    value !== "precondition_required" &&
    value !== "invalid_request" &&
    value !== "validation_failed" &&
    value !== "render_failed" &&
    value !== "unavailable"
  ) {
    throw new Error("problem.code is invalid");
  }
  return value;
}

function parseValidationDiagnostics(
  value: unknown,
  path: string,
): ValidationDiagnostic[] {
  if (!Array.isArray(value)) throw new Error(`${path} is invalid`);
  return value.map((diagnostic, index) =>
    parseValidationDiagnostic(diagnostic, index, path),
  );
}

function parseValidationDiagnostic(
  value: unknown,
  index: number,
  path: string,
): ValidationDiagnostic {
  const diagnosticPath = `${path}[${index}]`;
  const diagnostic = record(value, diagnosticPath);
  const target = record(diagnostic.target, `${diagnosticPath}.target`);
  const validationTarget = parseValidationTarget(
    target,
    `${diagnosticPath}.target`,
  );
  const span =
    diagnostic.span === undefined
      ? undefined
      : parseUtf8ByteSpan(diagnostic.span, diagnosticPath);
  const severity = diagnostic.severity;
  if (
    severity !== "error" &&
    severity !== "warning" &&
    severity !== "information" &&
    severity !== "hint"
  ) {
    throw new Error(`${diagnosticPath}.severity is invalid`);
  }
  return {
    code: text(diagnostic.code, `${diagnosticPath}.code`),
    severity,
    target: validationTarget,
    ...(span === undefined ? {} : { span }),
    message: text(diagnostic.message, `${diagnosticPath}.message`),
  };
}

function parseValidationTarget(
  target: Record<string, unknown>,
  path: string,
): ValidationTarget {
  const field = text(target.field, `${path}.field`);
  if (
    field === "source" ||
    field === "title" ||
    field === "body" ||
    field === "tags"
  ) {
    if (target.index !== undefined) {
      throw new Error(`${path}.index is not allowed`);
    }
    return { field };
  }
  if (field === "tag" || field === "acl_entry") {
    return {
      field,
      index: nonNegativeInteger(target.index, `${path}.index`),
    };
  }
  throw new Error(`${path}.field is invalid`);
}

function parseUtf8ByteSpan(
  value: unknown,
  diagnosticPath: string,
): ValidationDiagnostic["span"] {
  const span = record(value, `${diagnosticPath}.span`);
  if (span.unit !== "utf8_byte") {
    throw new Error(`${diagnosticPath}.span.unit is invalid`);
  }
  const start = nonNegativeInteger(span.start, `${diagnosticPath}.span.start`);
  const end = nonNegativeInteger(span.end, `${diagnosticPath}.span.end`);
  if (end < start) {
    throw new Error(`${diagnosticPath}.span.end is before span.start`);
  }
  return {
    start,
    end,
    unit: span.unit,
  };
}

export async function listNotes(apiBase: string): Promise<NoteListEntry[]> {
  return requestJson(`${apiBase}/notes`, undefined, parseNoteListEntries);
}

export async function readNote(
  apiBase: string,
  noteId: string,
  signal?: AbortSignal,
): Promise<Note> {
  return requestJson(
    `${apiBase}/notes/${encodeURIComponent(noteId)}`,
    { signal },
    parseNote,
  );
}

export async function readNoteView(
  apiBase: string,
  noteId: string,
  signal?: AbortSignal,
): Promise<NoteView> {
  return requestJson(
    `${apiBase}/notes/${encodeURIComponent(noteId)}/view`,
    { signal },
    parseNoteView,
  );
}

export async function createNote(
  apiBase: string,
  draft: NoteDraft,
): Promise<Note> {
  return requestJson(
    `${apiBase}/notes`,
    mutationRequest("POST", draft),
    parseNote,
  );
}

export async function updateNote(
  apiBase: string,
  noteId: string,
  draft: NoteDraft,
  expectedRevision: number,
): Promise<Note> {
  return requestJson(
    `${apiBase}/notes/${encodeURIComponent(noteId)}`,
    mutationRequest("PUT", draft, expectedRevision),
    parseNote,
  );
}

export async function previewNote(
  apiBase: string,
  draft: NoteDraft,
  signal?: AbortSignal,
): Promise<NotePreview> {
  return requestJson(
    `${apiBase}/notes/preview`,
    { ...mutationRequest("POST", draft), signal },
    parseNotePreview,
  );
}

export async function readNoteAcl(
  apiBase: string,
  noteId: string,
): Promise<{ entries: NoteAclGrant[]; revision: number }> {
  const response = await requestJsonResponse(
    `${apiBase}/notes/${encodeURIComponent(noteId)}/acl`,
    undefined,
    parseNoteAcl,
  );
  if (response.revision < 1) {
    throw new ApiError(200, {
      code: "invalid_response",
      message: "サーバーからETagを取得できませんでした。",
    });
  }
  return { ...response.value, revision: response.revision };
}

export async function replaceNoteAcl(
  apiBase: string,
  noteId: string,
  entries: NoteAclEntry[],
  expectedRevision: number,
): Promise<Note> {
  return requestJson(
    `${apiBase}/notes/${encodeURIComponent(noteId)}/acl`,
    mutationRequest("PUT", { entries }, expectedRevision),
    parseNote,
  );
}

function mutationRequest(
  method: "POST" | "PUT",
  body: unknown,
  expectedRevision?: number,
): RequestInit {
  const csrfToken = readCookie("marginalis_csrf");
  return {
    method,
    credentials: "same-origin",
    headers: {
      "content-type": "application/json",
      "x-csrf-token": csrfToken,
      ...(expectedRevision === undefined
        ? {}
        : { "if-match": `"rev-${expectedRevision}"` }),
    },
    body: JSON.stringify(body),
  };
}

async function requestJson<T>(
  url: string,
  init: RequestInit | undefined,
  parse: (value: unknown) => T,
): Promise<T> {
  return (await requestJsonResponse(url, init, parse)).value;
}

async function requestJsonResponse<T>(
  url: string,
  init: RequestInit | undefined,
  parse: (value: unknown) => T,
): Promise<{ value: T; revision: number }> {
  const response = await fetch(url, {
    credentials: "same-origin",
    ...init,
  });
  const payload: unknown = await response.json();
  if (!response.ok) {
    let problem: Problem;
    try {
      problem = parseProblem(payload);
    } catch {
      problem = {
        code: "invalid_response",
        message: "サーバーから解釈できない応答を受け取りました。",
      };
    }
    throw new ApiError(response.status, problem);
  }
  try {
    const value = parse(payload);
    const match = response.headers.get("etag")?.match(/^"rev-([1-9][0-9]*)"$/);
    return {
      value,
      revision: match ? positiveInteger(Number(match[1]), "response ETag") : 0,
    };
  } catch {
    throw new ApiError(response.status, {
      code: "invalid_response",
      message: "サーバーから解釈できない応答を受け取りました。",
    });
  }
}

function readCookie(name: string): string {
  for (const cookie of document.cookie.split(";")) {
    const [key, ...value] = cookie.trim().split("=");
    if (key === name) {
      return decodeURIComponent(value.join("="));
    }
  }
  return "";
}

function record(value: unknown, name: string): Record<string, unknown> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw new Error(`${name} is invalid`);
  }
  return value as Record<string, unknown>;
}
function text(value: unknown, name: string): string {
  if (typeof value !== "string") throw new Error(`${name} is invalid`);
  return value;
}
function integer(value: unknown, name: string): number {
  if (!Number.isSafeInteger(value)) throw new Error(`${name} is invalid`);
  return value as number;
}
function positiveInteger(value: unknown, name: string): number {
  const result = integer(value, name);
  if (result < 1) throw new Error(`${name} is invalid`);
  return result;
}
function nonNegativeInteger(value: unknown, name: string): number {
  const result = integer(value, name);
  if (result < 0) throw new Error(`${name} is invalid`);
  return result;
}
function textArray(value: unknown, name: string): string[] {
  if (!Array.isArray(value) || value.some((item) => typeof item !== "string")) {
    throw new Error(`${name} is invalid`);
  }
  return value;
}
