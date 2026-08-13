// REST APIのHTTPクライアント。契約(型と検証)はgenerated/contracts.tsを正本とする。
import {
  type BibliographyImportDecision,
  type BibliographyImportPreview,
  type BibliographyImportResult,
  type BibliographyImportSource,
  type BibliographyImportSourceInput,
  type BibliographyItem,
  type DeletedNoteListEntry,
  type MathMacroSettings,
  type McpClientAuthorization,
  type McpScopeCeiling,
  type McpScopeCeilingInput,
  type Note,
  type NoteAclEntry,
  type NoteAclGrant,
  type NoteDraft,
  type NoteGraph,
  type NoteListEntry,
  type NotePreview,
  type NoteReview,
  type NoteView,
  type Problem,
  parseBibliographyImportPreview,
  parseBibliographyImportResult,
  parseBibliographyImportSources,
  parseBibliographyItem,
  parseBibliographyItems,
  parseDeletedNoteListEntries,
  parseMathMacroSettings,
  parseMcpClientAuthorization,
  parseMcpClientAuthorizations,
  parseMcpScopeCeiling,
  parseNote,
  parseNoteAcl,
  parseNoteGraph,
  parseNoteListEntries,
  parseNotePreview,
  parseNoteReview,
  parseNoteView,
  parseProblem,
  parseWebhookSecret,
  parseWebhookSubscriptionCreated,
  parseWebhookSubscriptions,
  parseWebhookVerification,
  type WebhookSecret,
  type WebhookSubscription,
  type WebhookSubscriptionCreated,
  type WebhookSubscriptionDraft,
  type WebhookVerification,
} from "./generated/contracts";

export class ApiError extends Error {
  constructor(
    readonly status: number,
    readonly problem: Problem,
  ) {
    super(problem.message);
  }
}

export async function listNotes(
  apiBase: string,
  signal?: AbortSignal,
): Promise<NoteListEntry[]> {
  return requestJson(`${apiBase}/notes`, { signal }, parseNoteListEntries);
}

export async function listDeletedNotes(
  apiBase: string,
  signal?: AbortSignal,
): Promise<DeletedNoteListEntry[]> {
  return requestJson(
    `${apiBase}/notes/deleted`,
    { signal },
    parseDeletedNoteListEntries,
  );
}

export async function searchBibliography(
  apiBase: string,
  query = "",
  signal?: AbortSignal,
): Promise<BibliographyItem[]> {
  const suffix = query ? `?query=${encodeURIComponent(query)}` : "";
  return requestJson(
    `${apiBase}/bibliography${suffix}`,
    { signal },
    parseBibliographyItems,
  );
}

export async function addBibliographyItem(
  apiBase: string,
  cslJson: Record<string, unknown>,
): Promise<BibliographyItem> {
  return requestJson(
    `${apiBase}/bibliography`,
    mutationRequest("POST", { csl_json: cslJson }),
    parseBibliographyItem,
  );
}

export async function updateBibliographyItem(
  apiBase: string,
  itemId: string,
  cslJson: Record<string, unknown>,
  expectedRevision: number,
): Promise<BibliographyItem> {
  return requestJson(
    `${apiBase}/bibliography/${encodeURIComponent(itemId)}`,
    mutationRequest("PUT", { csl_json: cslJson }, expectedRevision),
    parseBibliographyItem,
  );
}

export async function deleteBibliographyItem(
  apiBase: string,
  itemId: string,
  expectedRevision: number,
): Promise<void> {
  const response = await fetch(
    `${apiBase}/bibliography/${encodeURIComponent(itemId)}`,
    mutationRequest("DELETE", undefined, expectedRevision),
  );
  if (!response.ok) {
    let problem: Problem;
    try {
      problem = parseProblem(await response.json());
    } catch {
      problem = {
        code: "invalid_response",
        message: "サーバーから解釈できない応答を受け取りました。",
      };
    }
    throw new ApiError(response.status, problem);
  }
}

export async function listBibliographyImportSources(
  apiBase: string,
  signal?: AbortSignal,
): Promise<BibliographyImportSource[]> {
  return requestJson(
    `${apiBase}/bibliography/import-sources`,
    { signal },
    parseBibliographyImportSources,
  );
}

export async function previewBibliographyImport(
  apiBase: string,
  source: BibliographyImportSourceInput,
  items: unknown[],
): Promise<BibliographyImportPreview> {
  return requestJson(
    `${apiBase}/bibliography/import-previews`,
    jsonPostRequest({ source, items }),
    parseBibliographyImportPreview,
  );
}

export async function applyBibliographyImport(
  apiBase: string,
  source: BibliographyImportSourceInput,
  items: unknown[],
  previewToken: string,
  decisions: BibliographyImportDecision[],
): Promise<BibliographyImportResult> {
  return requestJson(
    `${apiBase}/bibliography/imports`,
    mutationRequest("POST", {
      source,
      items,
      preview_token: previewToken,
      decisions,
    }),
    parseBibliographyImportResult,
  );
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

export async function readMathMacros(
  apiBase: string,
  signal?: AbortSignal,
): Promise<MathMacroSettings> {
  return requestJson(
    `${apiBase}/math-macros`,
    { signal },
    parseMathMacroSettings,
  );
}

export async function replaceMathMacros(
  apiBase: string,
  settings: MathMacroSettings,
): Promise<MathMacroSettings> {
  return requestJson(
    `${apiBase}/math-macros`,
    mutationRequest("PUT", settings),
    parseMathMacroSettings,
  );
}

export async function readMcpScopeCeiling(
  apiBase: string,
  signal?: AbortSignal,
): Promise<McpScopeCeiling> {
  return requestJson(
    `${apiBase}/mcp-scope-ceilings`,
    { signal },
    parseMcpScopeCeiling,
  );
}

export async function replaceMcpScopeCeiling(
  apiBase: string,
  settings: McpScopeCeilingInput,
): Promise<McpScopeCeiling> {
  return requestJson(
    `${apiBase}/mcp-scope-ceilings`,
    mutationRequest("PUT", settings),
    parseMcpScopeCeiling,
  );
}

export async function listMcpAuthorizations(
  apiBase: string,
  signal?: AbortSignal,
): Promise<McpClientAuthorization[]> {
  return requestJson(
    `${apiBase}/mcp-authorizations`,
    { signal },
    parseMcpClientAuthorizations,
  );
}

export async function replaceMcpClientScopeCeiling(
  apiBase: string,
  clientId: string,
  settings: McpScopeCeilingInput,
): Promise<McpClientAuthorization> {
  return requestJson(
    `${apiBase}/mcp-authorizations/${encodeURIComponent(clientId)}/scope-ceiling`,
    mutationRequest("PUT", settings),
    parseMcpClientAuthorization,
  );
}

export async function deleteMcpClientScopeCeiling(
  apiBase: string,
  clientId: string,
  revision: number,
): Promise<McpClientAuthorization> {
  return requestJson(
    `${apiBase}/mcp-authorizations/${encodeURIComponent(clientId)}/scope-ceiling?revision=${encodeURIComponent(String(revision))}`,
    mutationRequest("DELETE"),
    parseMcpClientAuthorization,
  );
}

export async function revokeMcpAuthorization(
  apiBase: string,
  clientId: string,
): Promise<void> {
  const response = await fetch(
    `${apiBase}/mcp-authorizations/${encodeURIComponent(clientId)}`,
    mutationRequest("DELETE"),
  );
  if (!response.ok) {
    let problem: Problem;
    try {
      problem = parseProblem(await response.json());
    } catch {
      problem = {
        code: "invalid_response",
        message: "サーバーから解釈できない応答を受け取りました。",
      };
    }
    throw new ApiError(response.status, problem);
  }
}

export async function listWebhookSubscriptions(
  apiBase: string,
  signal?: AbortSignal,
): Promise<WebhookSubscription[]> {
  return requestJson(
    `${apiBase}/webhooks`,
    { signal },
    parseWebhookSubscriptions,
  );
}

export async function createWebhookSubscription(
  apiBase: string,
  draft: WebhookSubscriptionDraft,
): Promise<WebhookSubscriptionCreated> {
  return requestJson(
    `${apiBase}/webhooks`,
    mutationRequest("POST", draft),
    parseWebhookSubscriptionCreated,
  );
}

export async function verifyWebhookSubscription(
  apiBase: string,
  subscriptionId: string,
): Promise<WebhookVerification> {
  return requestJson(
    `${apiBase}/webhooks/${encodeURIComponent(subscriptionId)}/verify`,
    mutationRequest("POST"),
    parseWebhookVerification,
  );
}

export async function regenerateWebhookSecret(
  apiBase: string,
  subscriptionId: string,
): Promise<WebhookSecret> {
  return requestJson(
    `${apiBase}/webhooks/${encodeURIComponent(subscriptionId)}/secret`,
    mutationRequest("POST"),
    parseWebhookSecret,
  );
}

export async function deleteWebhookSubscription(
  apiBase: string,
  subscriptionId: string,
): Promise<void> {
  await requestNoContent(
    `${apiBase}/webhooks/${encodeURIComponent(subscriptionId)}`,
    mutationRequest("DELETE"),
  );
}

export async function retryWebhookDelivery(
  apiBase: string,
  subscriptionId: string,
): Promise<void> {
  await requestNoContent(
    `${apiBase}/webhooks/${encodeURIComponent(subscriptionId)}/retry`,
    mutationRequest("POST"),
  );
}

export async function discardWebhookDelivery(
  apiBase: string,
  subscriptionId: string,
): Promise<void> {
  await requestNoContent(
    `${apiBase}/webhooks/${encodeURIComponent(subscriptionId)}/discard`,
    mutationRequest("POST"),
  );
}

/** 図に出す範囲。`origin`を指定すると、そこから`depth`本以内の線で辿れる範囲だけになる。 */
export interface NoteGraphScope {
  query?: string;
  origin?: string;
  depth?: number;
}

export async function readNoteGraph(
  apiBase: string,
  scope: NoteGraphScope = {},
  signal?: AbortSignal,
): Promise<NoteGraph> {
  const parameters = new URLSearchParams();
  if (scope.query) parameters.set("query", scope.query);
  if (scope.origin) {
    parameters.set("origin", scope.origin);
    parameters.set("depth", String(scope.depth ?? 1));
  }
  const suffix = parameters.size > 0 ? `?${parameters.toString()}` : "";
  return requestJson(
    `${apiBase}/notes/graph${suffix}`,
    { signal },
    parseNoteGraph,
  );
}

export async function createNote(
  apiBase: string,
  draft: NoteDraft,
): Promise<Note> {
  return requestJson(
    `${apiBase}/web/notes`,
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

export async function deleteNote(
  apiBase: string,
  noteId: string,
  expectedRevision: number,
): Promise<Note> {
  return requestJson(
    `${apiBase}/notes/${encodeURIComponent(noteId)}`,
    mutationRequest("DELETE", undefined, expectedRevision),
    parseNote,
  );
}

export async function restoreNote(
  apiBase: string,
  noteId: string,
  expectedRevision: number,
): Promise<Note> {
  return requestJson(
    `${apiBase}/notes/${encodeURIComponent(noteId)}/restore`,
    mutationRequest("POST", undefined, expectedRevision),
    parseNote,
  );
}

export async function previewNewNote(
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

export async function previewNoteUpdate(
  apiBase: string,
  noteId: string,
  draft: NoteDraft,
  signal?: AbortSignal,
): Promise<NotePreview> {
  return requestJson(
    `${apiBase}/notes/${encodeURIComponent(noteId)}/preview`,
    { ...mutationRequest("POST", draft), signal },
    parseNotePreview,
  );
}

export async function readNoteAcl(
  apiBase: string,
  noteId: string,
  signal?: AbortSignal,
): Promise<{ entries: NoteAclGrant[]; revision: number }> {
  const response = await requestJsonResponse(
    `${apiBase}/notes/${encodeURIComponent(noteId)}/acl`,
    { signal },
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

export async function markNoteReviewed(
  apiBase: string,
  noteId: string,
  expectedRevision: number,
): Promise<NoteReview> {
  return requestJson(
    `${apiBase}/notes/${encodeURIComponent(noteId)}/review`,
    mutationRequest("POST", undefined, expectedRevision),
    parseNoteReview,
  );
}

/** 204を返すendpointの共通処理。失敗はProblemとして解釈して投げる。 */
async function requestNoContent(url: string, init: RequestInit): Promise<void> {
  const response = await fetch(url, init);
  if (!response.ok) {
    let problem: Problem;
    try {
      problem = parseProblem(await response.json());
    } catch {
      problem = {
        code: "invalid_response",
        message: "サーバーから解釈できない応答を受け取りました。",
      };
    }
    throw new ApiError(response.status, problem);
  }
}

function mutationRequest(
  method: "POST" | "PUT" | "DELETE",
  body?: unknown,
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
    ...(body === undefined ? {} : { body: JSON.stringify(body) }),
  };
}

function jsonPostRequest(body: unknown): RequestInit {
  return {
    method: "POST",
    credentials: "same-origin",
    headers: { "content-type": "application/json" },
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

function positiveInteger(value: unknown, name: string): number {
  if (!Number.isSafeInteger(value) || (value as number) < 1) {
    throw new Error(`${name} is invalid`);
  }
  return value as number;
}
