export interface Note {
  note_id: string;
  title: string;
  body: string;
  tags: string[];
  created_at_ms: number;
  updated_at_ms: number;
  revision: number;
}

export interface NoteDraft {
  title: string;
  body: string;
  tags: string[];
}

export interface NotePreview {
  html: string;
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
  target: { field: string; index?: number };
  span?: { start: number; end: number; unit: "utf8_byte" };
  message: string;
}

export interface Problem {
  code: string;
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

export async function readNote(
  apiBase: string,
  noteId: string,
  signal?: AbortSignal,
): Promise<Note> {
  return requestJson(`${apiBase}/notes/${encodeURIComponent(noteId)}`, {
    signal,
  });
}

export async function createNote(
  apiBase: string,
  draft: NoteDraft,
): Promise<Note> {
  return requestJson(`${apiBase}/notes`, mutationRequest("POST", draft));
}

export async function updateNote(
  apiBase: string,
  noteId: string,
  draft: NoteDraft,
  expectedRevision: number,
): Promise<Note> {
  return requestJson(
    `${apiBase}/notes/${encodeURIComponent(noteId)}`,
    mutationRequest("PUT", {
      ...draft,
      expected_revision: expectedRevision,
    }),
  );
}

export async function previewNote(
  apiBase: string,
  draft: NoteDraft,
  signal?: AbortSignal,
): Promise<NotePreview> {
  return requestJson(`${apiBase}/notes/preview`, {
    ...mutationRequest("POST", draft),
    signal,
  });
}

export async function readNoteAcl(
  apiBase: string,
  noteId: string,
): Promise<{ entries: NoteAclGrant[] }> {
  return requestJson(`${apiBase}/notes/${encodeURIComponent(noteId)}/acl`);
}

export async function replaceNoteAcl(
  apiBase: string,
  noteId: string,
  entries: NoteAclEntry[],
  expectedRevision: number,
): Promise<Note> {
  return requestJson(
    `${apiBase}/notes/${encodeURIComponent(noteId)}/acl`,
    mutationRequest("PUT", { entries, expected_revision: expectedRevision }),
  );
}

function mutationRequest(method: "POST" | "PUT", body: unknown): RequestInit {
  const csrfToken = readCookie("marginalis_csrf");
  return {
    method,
    credentials: "same-origin",
    headers: {
      "content-type": "application/json",
      "x-csrf-token": csrfToken,
    },
    body: JSON.stringify(body),
  };
}

async function requestJson<T>(url: string, init?: RequestInit): Promise<T> {
  const response = await fetch(url, {
    credentials: "same-origin",
    ...init,
  });
  const payload: unknown = await response.json();
  if (!response.ok) {
    throw new ApiError(response.status, problemFrom(payload));
  }
  return payload as T;
}

function problemFrom(value: unknown): Problem {
  if (
    typeof value === "object" &&
    value !== null &&
    "code" in value &&
    typeof value.code === "string" &&
    "message" in value &&
    typeof value.message === "string"
  ) {
    return value as Problem;
  }
  return {
    code: "invalid_response",
    message: "サーバーから解釈できない応答を受け取りました。",
  };
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
