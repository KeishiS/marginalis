// 正本はcrates/marginalis-contract/srcにあります。生成先では直接編集しないでください。
// 公開契約を変える場合は、正本とRustの契約を同時に編集してください。
export interface Note {
  note_id: string;
  title: string;
  source: string;
  tags: string[];
  created_at_ms: number;
  updated_at_ms: number;
  revision: number;
  created_via: NoteCreationSource;
  review_status: NoteReviewStatus;
  reviewed_revision: number | null;
  reviewed_at_ms: number | null;
}
export type NoteCreationSource = "web" | "rest" | "mcp" | "unknown";
export type NoteReviewStatus = "unknown" | "pending" | "reviewed";
export interface NoteSummary {
  note_id: string;
  title: string;
  tags: string[];
  updated_at_ms: number;
  revision: number;
  created_via: NoteCreationSource;
  review_status: NoteReviewStatus;
  reviewed_revision: number | null;
  reviewed_at_ms: number | null;
}
export type NoteAccess = "read" | "edit" | "manage";
export interface NoteListEntry extends NoteSummary {
  access: NoteAccess;
}
export interface NoteReview {
  note_id: string;
  current_revision: number;
  status: NoteReviewStatus;
  reviewed_revision: number | null;
  reviewed_at_ms: number | null;
  reviewer_issuer: string | null;
  reviewer_subject: string | null;
}
export interface DeletedNoteListEntry {
  note_id: string;
  title: string;
  deleted_at_ms: number;
  purge_at_ms: number;
  revision: number;
}
export interface RelatedNotes {
  outgoing: NoteSummary[];
  incoming: NoteSummary[];
}
export interface MathMacro {
  name: string;
  replacement: string;
  argument_count: number;
}
export interface MathMacroSettings {
  macros: MathMacro[];
  revision: number;
}
export interface McpScopeCeilingInput {
  scopes: string[];
  revision: number;
}
export interface McpScopeCeiling extends McpScopeCeilingInput {
  supported_scopes: string[];
}
export interface McpClientAuthorization {
  client_id: string;
  display_name: string;
  registration_method: "metadata_document" | "dynamic";
  granted_scopes: string[];
  scope_ceiling_configured: boolean;
  scope_ceiling: string[];
  scope_ceiling_revision: number;
  authorized_at_ms: number;
  last_used_at_ms: number | null;
  active: boolean;
}
export interface NoteView {
  note: Note;
  access: NoteAccess;
  html: string;
  math_macros: MathMacro[];
  related: RelatedNotes;
}
export interface NoteGraphNote {
  note_id: string;
  title: string;
  tags: string[];
  updated_at_ms: number;
}
export interface NoteGraphWork {
  citation_key: string;
  title: string | null;
}
export interface NoteGraphReference {
  source_note_id: string;
  target_note_id: string;
}
export interface NoteGraphCitation {
  source_note_id: string;
  citation_key: string;
}
export interface NoteGraph {
  notes: NoteGraphNote[];
  works: NoteGraphWork[];
  references: NoteGraphReference[];
  citations: NoteGraphCitation[];
}
export interface NoteDraft {
  source: string;
}
export interface BibliographyItem {
  item_id: string;
  citation_key: string;
  csl_json: Record<string, unknown>;
  created_at_ms: number;
  updated_at_ms: number;
  revision: number;
}
export type BibliographyImportSourceInput =
  | { kind: "new"; display_name: string }
  | { kind: "existing"; source_id: string };
export interface BibliographyImportSource {
  source_id: string;
  method: "csl_json_file";
  display_name: string;
  revision: number;
  created_at_ms: number;
  last_imported_at_ms: number;
}
export type BibliographyImportClassification =
  | "create"
  | "update_from_external"
  | "unchanged"
  | "keep_local"
  | "conflict"
  | "duplicate_candidate"
  | "rejected";
export interface BibliographyImportCandidate {
  item_id: string;
  citation_key: string;
  title: string | null;
  revision: number;
  matched_by: string[];
}
export interface BibliographyImportEntry {
  position: number;
  external_item_id: string | null;
  citation_key: string | null;
  classification: BibliographyImportClassification;
  item_id: string | null;
  item_revision: number | null;
  current_csl_json: Record<string, unknown> | null;
  candidates: BibliographyImportCandidate[];
  rejection_code: string | null;
}
export interface BibliographyImportPreview {
  source_id: string | null;
  source_revision: number | null;
  preview_token: string;
  entries: BibliographyImportEntry[];
}
export type BibliographyImportDecisionAction =
  | "apply_suggested"
  | "create_separate"
  | "keep_local"
  | "use_external"
  | "link_existing_keep_local"
  | "link_existing_use_external"
  | "exclude";
export interface BibliographyImportDecision {
  position: number;
  action: BibliographyImportDecisionAction;
  candidate_item_id: string | null;
}
export interface BibliographyImportResult {
  source_id: string;
  source_revision: number;
  created: number;
  updated: number;
  kept: number;
  excluded: number;
}
export interface NotePreview {
  html: string;
  math_macros: MathMacro[];
  diagnostics: NoteDiagnostic[];
}
export type NotePermission = "read" | "edit";
export interface NoteAclEntry {
  subject: string;
  permission: NotePermission;
}
export interface NoteAclGrant extends NoteAclEntry {
  issuer: string;
}
export interface NoteDiagnostic {
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
  | "retention_expired"
  | "precondition_required"
  | "invalid_request"
  | "validation_failed"
  | "render_failed"
  | "unavailable";
export interface Problem {
  code: ProblemCode | "invalid_response" | "network_error";
  message: string;
  diagnostics?: NoteDiagnostic[];
}

export class ApiError extends Error {
  constructor(
    readonly status: number,
    readonly problem: Problem,
  ) {
    super(problem.message);
  }
}

/** サーバーが初期HTMLへ埋め込む起動設定。 */
export interface ApplicationConfig {
  apiBase: string;
  basePath: string;
  path: string;
  search: string;
  styleNonce: string;
}

/**
 * 起動設定を検査して読み取る。
 *
 * REST応答と同じく、解釈できない値は例外として扱う。項目の欠落や型の誤りを
 * 型アサーションで見逃さない。
 */
export function parseApplicationConfig(value: unknown): ApplicationConfig {
  const object = record(value, "application config");
  return {
    apiBase: text(object.apiBase, "application config.apiBase"),
    basePath: text(object.basePath, "application config.basePath"),
    path: text(object.path, "application config.path"),
    search: text(object.search, "application config.search"),
    styleNonce: text(object.styleNonce, "application config.styleNonce"),
  };
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
    ...parseNoteProvenance(object, "note"),
  };
}

export function parseBibliographyItem(value: unknown): BibliographyItem {
  const object = record(value, "bibliography item");
  return {
    item_id: text(object.item_id, "bibliography item.item_id"),
    citation_key: text(object.citation_key, "bibliography item.citation_key"),
    csl_json: record(object.csl_json, "bibliography item.csl_json"),
    created_at_ms: integer(
      object.created_at_ms,
      "bibliography item.created_at_ms",
    ),
    updated_at_ms: integer(
      object.updated_at_ms,
      "bibliography item.updated_at_ms",
    ),
    revision: positiveInteger(object.revision, "bibliography item.revision"),
  };
}

export function parseBibliographyItems(value: unknown): BibliographyItem[] {
  if (!Array.isArray(value)) throw new Error("bibliography items are invalid");
  return value.map(parseBibliographyItem);
}

function parseBibliographyImportSource(
  value: unknown,
): BibliographyImportSource {
  const object = record(value, "bibliography import source");
  if (object.method !== "csl_json_file") {
    throw new Error("bibliography import source.method is invalid");
  }
  return {
    source_id: text(object.source_id, "bibliography import source.source_id"),
    method: object.method,
    display_name: text(
      object.display_name,
      "bibliography import source.display_name",
    ),
    revision: positiveInteger(
      object.revision,
      "bibliography import source.revision",
    ),
    created_at_ms: integer(
      object.created_at_ms,
      "bibliography import source.created_at_ms",
    ),
    last_imported_at_ms: integer(
      object.last_imported_at_ms,
      "bibliography import source.last_imported_at_ms",
    ),
  };
}

function parseBibliographyImportSources(
  value: unknown,
): BibliographyImportSource[] {
  if (!Array.isArray(value)) {
    throw new Error("bibliography import sources are invalid");
  }
  return value.map(parseBibliographyImportSource);
}

const IMPORT_CLASSIFICATIONS: BibliographyImportClassification[] = [
  "create",
  "update_from_external",
  "unchanged",
  "keep_local",
  "conflict",
  "duplicate_candidate",
  "rejected",
];

function parseBibliographyImportCandidate(
  value: unknown,
): BibliographyImportCandidate {
  const object = record(value, "bibliography import candidate");
  return {
    item_id: text(object.item_id, "bibliography import candidate.item_id"),
    citation_key: text(
      object.citation_key,
      "bibliography import candidate.citation_key",
    ),
    title:
      object.title === null
        ? null
        : text(object.title, "bibliography import candidate.title"),
    revision: positiveInteger(
      object.revision,
      "bibliography import candidate.revision",
    ),
    matched_by: textArray(
      object.matched_by,
      "bibliography import candidate.matched_by",
    ),
  };
}

function parseBibliographyImportEntry(value: unknown): BibliographyImportEntry {
  const object = record(value, "bibliography import entry");
  if (
    typeof object.classification !== "string" ||
    !IMPORT_CLASSIFICATIONS.includes(
      object.classification as BibliographyImportClassification,
    )
  ) {
    throw new Error("bibliography import entry.classification is invalid");
  }
  if (!Array.isArray(object.candidates)) {
    throw new Error("bibliography import entry.candidates is invalid");
  }
  return {
    position: nonNegativeInteger(
      object.position,
      "bibliography import entry.position",
    ),
    external_item_id:
      object.external_item_id === null
        ? null
        : text(
            object.external_item_id,
            "bibliography import entry.external_item_id",
          ),
    citation_key:
      object.citation_key === null
        ? null
        : text(object.citation_key, "bibliography import entry.citation_key"),
    classification: object.classification as BibliographyImportClassification,
    item_id:
      object.item_id === null
        ? null
        : text(object.item_id, "bibliography import entry.item_id"),
    item_revision:
      object.item_revision === null
        ? null
        : positiveInteger(
            object.item_revision,
            "bibliography import entry.item_revision",
          ),
    current_csl_json:
      object.current_csl_json === null
        ? null
        : record(
            object.current_csl_json,
            "bibliography import entry.current_csl_json",
          ),
    candidates: object.candidates.map(parseBibliographyImportCandidate),
    rejection_code:
      object.rejection_code === null
        ? null
        : text(
            object.rejection_code,
            "bibliography import entry.rejection_code",
          ),
  };
}

function parseBibliographyImportPreview(
  value: unknown,
): BibliographyImportPreview {
  const object = record(value, "bibliography import preview");
  if (!Array.isArray(object.entries)) {
    throw new Error("bibliography import preview.entries is invalid");
  }
  const previewToken = text(
    object.preview_token,
    "bibliography import preview.preview_token",
  );
  if (!/^[0-9a-f]{64}$/.test(previewToken)) {
    throw new Error("bibliography import preview.preview_token is invalid");
  }
  return {
    source_id:
      object.source_id === null
        ? null
        : text(object.source_id, "bibliography import preview.source_id"),
    source_revision:
      object.source_revision === null
        ? null
        : positiveInteger(
            object.source_revision,
            "bibliography import preview.source_revision",
          ),
    preview_token: previewToken,
    entries: object.entries.map(parseBibliographyImportEntry),
  };
}

function parseBibliographyImportResult(
  value: unknown,
): BibliographyImportResult {
  const object = record(value, "bibliography import result");
  return {
    source_id: text(object.source_id, "bibliography import result.source_id"),
    source_revision: positiveInteger(
      object.source_revision,
      "bibliography import result.source_revision",
    ),
    created: nonNegativeInteger(
      object.created,
      "bibliography import result.created",
    ),
    updated: nonNegativeInteger(
      object.updated,
      "bibliography import result.updated",
    ),
    kept: nonNegativeInteger(object.kept, "bibliography import result.kept"),
    excluded: nonNegativeInteger(
      object.excluded,
      "bibliography import result.excluded",
    ),
  };
}

export function parseNotePreview(value: unknown): NotePreview {
  const object = record(value, "preview");
  return {
    html: text(object.html, "preview.html"),
    math_macros: parseMathMacros(object.math_macros, "preview.math_macros"),
    diagnostics: parseNoteDiagnostics(
      object.diagnostics,
      "preview.diagnostics",
    ),
  };
}

function parseMathMacro(value: unknown, label: string): MathMacro {
  const object = record(value, label);
  const argumentCount = integer(
    object.argument_count,
    `${label}.argument_count`,
  );
  if (argumentCount < 0 || argumentCount > 9) {
    throw new Error(`${label}.argument_count is invalid`);
  }
  return {
    name: text(object.name, `${label}.name`),
    replacement: text(object.replacement, `${label}.replacement`),
    argument_count: argumentCount,
  };
}

function parseMathMacros(value: unknown, label: string): MathMacro[] {
  return array(value, label).map((entry, index) =>
    parseMathMacro(entry, `${label}[${index}]`),
  );
}

export function parseMathMacroSettings(value: unknown): MathMacroSettings {
  const object = record(value, "math macro settings");
  const revision = integer(object.revision, "math macro settings.revision");
  if (revision < 0) throw new Error("math macro settings.revision is invalid");
  return {
    macros: parseMathMacros(object.macros, "math macro settings.macros"),
    revision,
  };
}

export function parseMcpScopeCeiling(value: unknown): McpScopeCeiling {
  const object = record(value, "MCP scope ceiling");
  const revision = integer(object.revision, "MCP scope ceiling.revision");
  if (revision < 0) throw new Error("MCP scope ceiling.revision is invalid");
  return {
    supported_scopes: textArray(
      object.supported_scopes,
      "MCP scope ceiling.supported_scopes",
    ),
    scopes: textArray(object.scopes, "MCP scope ceiling.scopes"),
    revision,
  };
}

export function parseMcpClientAuthorization(
  value: unknown,
): McpClientAuthorization {
  const object = record(value, "MCP client authorization");
  const registrationMethod = text(
    object.registration_method,
    "MCP client authorization.registration_method",
  );
  if (
    registrationMethod !== "metadata_document" &&
    registrationMethod !== "dynamic"
  ) {
    throw new Error("MCP client authorization.registration_method is invalid");
  }
  const revision = integer(
    object.scope_ceiling_revision,
    "MCP client authorization.scope_ceiling_revision",
  );
  if (revision < 0) {
    throw new Error(
      "MCP client authorization.scope_ceiling_revision is invalid",
    );
  }
  if (typeof object.scope_ceiling_configured !== "boolean") {
    throw new Error(
      "MCP client authorization.scope_ceiling_configured is invalid",
    );
  }
  if (typeof object.active !== "boolean") {
    throw new Error("MCP client authorization.active is invalid");
  }
  return {
    client_id: text(object.client_id, "MCP client authorization.client_id"),
    display_name: text(
      object.display_name,
      "MCP client authorization.display_name",
    ),
    registration_method: registrationMethod,
    granted_scopes: textArray(
      object.granted_scopes,
      "MCP client authorization.granted_scopes",
    ),
    scope_ceiling_configured: object.scope_ceiling_configured,
    scope_ceiling: textArray(
      object.scope_ceiling,
      "MCP client authorization.scope_ceiling",
    ),
    scope_ceiling_revision: revision,
    authorized_at_ms: integer(
      object.authorized_at_ms,
      "MCP client authorization.authorized_at_ms",
    ),
    last_used_at_ms:
      object.last_used_at_ms === null
        ? null
        : integer(
            object.last_used_at_ms,
            "MCP client authorization.last_used_at_ms",
          ),
    active: object.active,
  };
}

export function parseMcpClientAuthorizations(
  value: unknown,
): McpClientAuthorization[] {
  return array(value, "MCP client authorizations").map(
    parseMcpClientAuthorization,
  );
}

export function parseNoteSummary(value: unknown): NoteSummary {
  const object = record(value, "note summary");
  return {
    note_id: text(object.note_id, "note summary.note_id"),
    title: text(object.title, "note summary.title"),
    tags: textArray(object.tags, "note summary.tags"),
    updated_at_ms: integer(object.updated_at_ms, "note summary.updated_at_ms"),
    revision: positiveInteger(object.revision, "note summary.revision"),
    ...parseNoteProvenance(object, "note summary"),
  };
}

function parseNoteProvenance(
  object: Record<string, unknown>,
  path: string,
): Pick<
  NoteSummary,
  "created_via" | "review_status" | "reviewed_revision" | "reviewed_at_ms"
> {
  const createdVia = object.created_via;
  if (
    createdVia !== "web" &&
    createdVia !== "rest" &&
    createdVia !== "mcp" &&
    createdVia !== "unknown"
  ) {
    throw new Error(`${path}.created_via is invalid`);
  }
  const reviewStatus = object.review_status;
  if (
    reviewStatus !== "unknown" &&
    reviewStatus !== "pending" &&
    reviewStatus !== "reviewed"
  ) {
    throw new Error(`${path}.review_status is invalid`);
  }
  return {
    created_via: createdVia,
    review_status: reviewStatus,
    reviewed_revision:
      object.reviewed_revision === null
        ? null
        : positiveInteger(
            object.reviewed_revision,
            `${path}.reviewed_revision`,
          ),
    reviewed_at_ms:
      object.reviewed_at_ms === null
        ? null
        : integer(object.reviewed_at_ms, `${path}.reviewed_at_ms`),
  };
}

export function parseNoteReview(value: unknown): NoteReview {
  const object = record(value, "note review");
  const provenance = parseNoteProvenance(
    {
      created_via: "unknown",
      review_status: object.status,
      reviewed_revision: object.reviewed_revision,
      reviewed_at_ms: object.reviewed_at_ms,
    },
    "note review",
  );
  return {
    note_id: text(object.note_id, "note review.note_id"),
    current_revision: positiveInteger(
      object.current_revision,
      "note review.current_revision",
    ),
    status: provenance.review_status,
    reviewed_revision: provenance.reviewed_revision,
    reviewed_at_ms: provenance.reviewed_at_ms,
    reviewer_issuer:
      object.reviewer_issuer === null
        ? null
        : text(object.reviewer_issuer, "note review.reviewer_issuer"),
    reviewer_subject:
      object.reviewer_subject === null
        ? null
        : text(object.reviewer_subject, "note review.reviewer_subject"),
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

export function parseDeletedNoteListEntries(
  value: unknown,
): DeletedNoteListEntry[] {
  if (!Array.isArray(value)) {
    throw new Error("deleted note list entries are invalid");
  }
  return value.map((entry, index) => {
    const object = record(entry, `deleted note list entries[${index}]`);
    return {
      note_id: text(
        object.note_id,
        `deleted note list entries[${index}].note_id`,
      ),
      title: text(object.title, `deleted note list entries[${index}].title`),
      deleted_at_ms: integer(
        object.deleted_at_ms,
        `deleted note list entries[${index}].deleted_at_ms`,
      ),
      purge_at_ms: integer(
        object.purge_at_ms,
        `deleted note list entries[${index}].purge_at_ms`,
      ),
      revision: positiveInteger(
        object.revision,
        `deleted note list entries[${index}].revision`,
      ),
    };
  });
}

export function parseNoteGraph(value: unknown): NoteGraph {
  const object = record(value, "note graph");
  return {
    notes: array(object.notes, "note graph.notes").map((entry) => {
      const note = record(entry, "note graph.notes[]");
      return {
        note_id: text(note.note_id, "note graph.notes[].note_id"),
        title: text(note.title, "note graph.notes[].title"),
        tags: textArray(note.tags, "note graph.notes[].tags"),
        updated_at_ms: integer(
          note.updated_at_ms,
          "note graph.notes[].updated_at_ms",
        ),
      };
    }),
    works: array(object.works, "note graph.works").map((entry) => {
      const work = record(entry, "note graph.works[]");
      return {
        citation_key: text(
          work.citation_key,
          "note graph.works[].citation_key",
        ),
        title:
          work.title === null
            ? null
            : text(work.title, "note graph.works[].title"),
      };
    }),
    references: array(object.references, "note graph.references").map(
      (entry) => {
        const edge = record(entry, "note graph.references[]");
        return {
          source_note_id: text(
            edge.source_note_id,
            "note graph.references[].source_note_id",
          ),
          target_note_id: text(
            edge.target_note_id,
            "note graph.references[].target_note_id",
          ),
        };
      },
    ),
    citations: array(object.citations, "note graph.citations").map((entry) => {
      const edge = record(entry, "note graph.citations[]");
      return {
        source_note_id: text(
          edge.source_note_id,
          "note graph.citations[].source_note_id",
        ),
        citation_key: text(
          edge.citation_key,
          "note graph.citations[].citation_key",
        ),
      };
    }),
  };
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
    math_macros: parseMathMacros(object.math_macros, "note view.math_macros"),
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
      ? parseNoteDiagnostics(object.diagnostics, "problem.diagnostics")
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
    value !== "retention_expired" &&
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

function parseNoteDiagnostics(value: unknown, path: string): NoteDiagnostic[] {
  if (!Array.isArray(value)) throw new Error(`${path} is invalid`);
  return value.map((diagnostic, index) =>
    parseNoteDiagnostic(diagnostic, index, path),
  );
}

function parseNoteDiagnostic(
  value: unknown,
  index: number,
  path: string,
): NoteDiagnostic {
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
): NoteDiagnostic["span"] {
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
function array(value: unknown, name: string): unknown[] {
  if (!Array.isArray(value)) throw new Error(`${name} is invalid`);
  return value;
}
function textArray(value: unknown, name: string): string[] {
  if (!Array.isArray(value) || value.some((item) => typeof item !== "string")) {
    throw new Error(`${name} is invalid`);
  }
  return value;
}
