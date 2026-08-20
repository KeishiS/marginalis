// このファイルはmarginalis-contractから生成します。直接編集しないでください。
// 再生成: cargo run -p marginalis-contract --bin generate
/* eslint-disable */
export interface ApplicationConfig {
  apiBase: string;
  basePath: string;
  path: string;
  search: string;
  styleNonce: string;
}
export type AttachmentMediaType = "image/png" | "image/jpeg" | "image/webp";
export interface BibliographyImportApplyInput {
  decisions: BibliographyImportDecision[];
  items: unknown[];
  preview_token: string;
  source: BibliographyImportSourceInput;
}
export interface BibliographyImportCandidate {
  citation_key: string;
  item_id: string;
  matched_by: string[];
  revision: number;
  title: string | null;
}
export type BibliographyImportClassification = "create" | "update_from_external" | "unchanged" | "keep_local" | "conflict" | "duplicate_candidate" | "rejected";
export interface BibliographyImportDecision {
  action: BibliographyImportDecisionAction;
  candidate_item_id: string | null;
  position: number;
}
export type BibliographyImportDecisionAction = "apply_suggested" | "create_separate" | "keep_local" | "use_external" | "link_existing_keep_local" | "link_existing_use_external" | "exclude";
export interface BibliographyImportEntry {
  candidates: BibliographyImportCandidate[];
  citation_key: string | null;
  classification: BibliographyImportClassification;
  current_csl_json: Record<string, unknown> | null;
  external_item_id: string | null;
  item_id: string | null;
  item_revision: number | null;
  position: number;
  rejection_code: string | null;
}
export interface BibliographyImportPreview {
  entries: BibliographyImportEntry[];
  preview_token: string;
  source_id: string | null;
  source_revision: number | null;
}
export interface BibliographyImportPreviewInput {
  items: unknown[];
  source: BibliographyImportSourceInput;
}
export interface BibliographyImportResult {
  created: number;
  excluded: number;
  kept: number;
  source_id: string;
  source_revision: number;
  updated: number;
}
export interface BibliographyImportSource {
  created_at_ms: number;
  display_name: string;
  last_imported_at_ms: number;
  method: string;
  revision: number;
  source_id: string;
}
export type BibliographyImportSourceInput = { display_name: string; kind: "new"; } | { kind: "existing"; source_id: string; };
export interface BibliographyItem {
  citation_key: string;
  created_at_ms: number;
  csl_json: Record<string, unknown>;
  item_id: string;
  revision: number;
  updated_at_ms: number;
}
export interface BibliographyItemInput {
  csl_json: Record<string, unknown>;
}
export interface DeletedNoteListEntry {
  deleted_at_ms: number;
  note_id: string;
  purge_at_ms: number;
  revision: number;
  title: string;
}
export type DiagnosticSeverity = "error" | "warning" | "information" | "hint";
export interface Health {
  api_version: string;
  status: string;
}
export interface MathMacro {
  argument_count: number;
  name: string;
  replacement: string;
}
export interface MathMacroSettings {
  macros: MathMacro[];
  revision: number;
}
export interface McpClientAuthorization {
  active: boolean;
  authorized_at_ms: number;
  client_id: string;
  display_name: string;
  granted_scopes: string[];
  last_used_at_ms: number | null;
  registration_method: string;
  scope_ceiling: string[];
  scope_ceiling_configured: boolean;
  scope_ceiling_revision: number;
}
export interface McpScopeCeiling {
  revision: number;
  scopes: string[];
  supported_scopes: string[];
}
export interface McpScopeCeilingInput {
  revision: number;
  scopes: string[];
}
export interface Note {
  created_at_ms: number;
  created_via: NoteCreationSource;
  note_id: string;
  review_status: NoteReviewStatus;
  reviewed_at_ms: number | null;
  reviewed_revision: number | null;
  revision: number;
  source: string;
  tags: string[];
  title: string;
  updated_at_ms: number;
}
export type NoteAccess = "read" | "edit" | "manage";
export interface NoteAcl {
  entries: NoteAclGrant[];
}
export interface NoteAclEntry {
  issuer: string;
  permission: NotePermission;
  subject: string;
}
export interface NoteAclGrant {
  issuer: string;
  permission: NotePermission;
  subject: string;
}
export interface NoteAclUpdate {
  entries: NoteAclEntry[];
}
export interface NoteAttachment {
  attachment_id: string;
  byte_length: number;
  created_at_ms: number;
  created_by_issuer: string;
  created_by_subject: string;
  file_name: string;
  media_type: AttachmentMediaType;
  sha256: string;
  source_target: string;
}
export type NoteCreationSource = "web" | "rest" | "mcp" | "unknown";
export interface NoteDiagnostic {
  code: string;
  message: string;
  position?: NoteSourcePosition | null;
  severity: DiagnosticSeverity;
  span?: Utf8ByteSpan | null;
  target: NoteValidationTarget;
}
export interface NoteDraft {
  source: string;
}
export interface NoteGraph {
  citations: NoteGraphCitation[];
  notes: NoteGraphNote[];
  references: NoteGraphReference[];
  works: NoteGraphWork[];
}
export interface NoteGraphCitation {
  citation_key: string;
  source_note_id: string;
}
export interface NoteGraphNote {
  note_id: string;
  tags: string[];
  title: string;
  updated_at_ms: number;
}
export interface NoteGraphReference {
  source_note_id: string;
  target_note_id: string;
}
export interface NoteGraphWork {
  citation_key: string;
  title: string | null;
}
export interface NoteListEntry {
  access: NoteAccess;
  created_via: NoteCreationSource;
  note_id: string;
  review_status: NoteReviewStatus;
  reviewed_at_ms: number | null;
  reviewed_revision: number | null;
  revision: number;
  tags: string[];
  title: string;
  updated_at_ms: number;
}
export type NotePermission = "read" | "edit";
export interface NotePreview {
  diagnostics: NoteDiagnostic[];
  html: string;
  math_macros: MathMacro[];
  spans: NoteSourceSpan[];
}
export interface NoteReview {
  current_revision: number;
  note_id: string;
  reviewed_at_ms: number | null;
  reviewed_revision: number | null;
  reviewer_issuer: string | null;
  reviewer_subject: string | null;
  status: NoteReviewStatus;
}
export type NoteReviewStatus = "unknown" | "pending" | "reviewed";
export interface NoteRevision {
  access: NoteAccess;
  changed_by_issuer: string;
  changed_by_subject: string;
  deleted_at_ms: number | null;
  kind: NoteRevisionKind;
  note: Note;
}
export interface NoteRevisionDiff {
  from_revision: number;
  to_revision: number;
  unified_diff: string;
}
export type NoteRevisionKind = "created" | "content_updated" | "acl_updated" | "reviewed" | "deleted" | "restored" | "history_restored" | "imported";
export interface NoteRevisionSummary {
  changed_at_ms: number;
  changed_by_issuer: string;
  changed_by_subject: string;
  kind: NoteRevisionKind;
  revision: number;
}
export interface NoteSourcePosition {
  column: number;
  line: number;
}
export interface NoteSourceSpan {
  content_span?: Utf8ByteSpan | null;
  kind: NoteSourceSpanKind;
  level?: number | null;
  marker_spans?: Utf8ByteSpan[];
  span: Utf8ByteSpan;
}
export type NoteSourceSpanKind = "document_title" | "heading" | "document_attribute" | "anchor" | "strong" | "emphasis" | "highlight" | "subscript" | "superscript" | "monospace" | "link" | "cross_reference" | "citation" | "inline_math" | "math_block" | "source_block" | "literal_block" | "quote" | "example" | "admonition" | "table" | "list_item";
export interface NoteSummary {
  created_via: NoteCreationSource;
  note_id: string;
  review_status: NoteReviewStatus;
  reviewed_at_ms: number | null;
  reviewed_revision: number | null;
  revision: number;
  tags: string[];
  title: string;
  updated_at_ms: number;
}
export type NoteValidationTarget = { field: "source"; } | { field: "title"; } | { field: "body"; } | { field: "tag"; index: number; } | { field: "tags"; } | { field: "acl_entry"; index: number; };
export interface NoteView {
  access: NoteAccess;
  html: string;
  math_macros: MathMacro[];
  note: Note;
  related: RelatedNotes;
}
export type ProblemCode = "authentication_required" | "authentication_unavailable" | "csrf_rejected" | "csrf_required" | "csrf_invalid" | "same_origin_required" | "origin_not_allowed" | "not_found" | "forbidden" | "conflict" | "retention_expired" | "invalid_sync_cursor" | "sync_cursor_expired" | "precondition_required" | "invalid_request" | "patch_rejected" | "validation_failed" | "advisories_rejected" | "render_failed" | "unavailable";
export interface RelatedNotes {
  incoming: NoteSummary[];
  outgoing: NoteSummary[];
}
export interface Session {
  issuer: string;
  subject: string;
}
export interface Utf8ByteSpan {
  end: number;
  start: number;
  unit: Utf8ByteUnit;
}
export type Utf8ByteUnit = "utf8_byte";
export type WebhookEventKind = "note.created" | "note.updated" | "note.deleted" | "note.restored" | "bibliography_item.created" | "bibliography_item.updated" | "bibliography_item.deleted";
export interface WebhookSecret {
  secret: string;
}
export interface WebhookSubscription {
  created_at_ms: number;
  disabled_reason: "delivery_exhausted" | "destination_rejected" | "owner_disabled" | null;
  event_kinds: WebhookEventKind[];
  last_attempted_at_ms: number | null;
  last_failure: "non_success_status" | "connect_failed" | "timed_out" | "destination_rejected" | null;
  next_attempt_at_ms: number | null;
  pending_count: number;
  revision: number;
  state: WebhookSubscriptionState;
  subscription_id: string;
  updated_at_ms: number;
  url: string;
}
export interface WebhookSubscriptionCreated {
  secret: string;
  subscription: WebhookSubscription;
}
export interface WebhookSubscriptionDraft {
  event_kinds: WebhookEventKind[];
  url: string;
}
export type WebhookSubscriptionState = "pending_challenge" | "active" | "disabled";
export interface WebhookVerification {
  failure: "non_success_status" | "connect_failed" | "timed_out" | "destination_rejected" | null;
  verified: boolean;
}
export type ValidationTarget = NoteValidationTarget;
/** サーバーの失敗応答。クライアントが合成する応答不正・通信失敗のcodeを含む。 */
export interface Problem {
  code: ProblemCode | "invalid_response" | "network_error";
  message: string;
  diagnostics?: NoteDiagnostic[];
}
/** テンプレートノートを識別するタグ。正本はmarginalis-domainのNOTE_TEMPLATE_TAG。 */
export const NOTE_TEMPLATE_TAG = "テンプレート";
export const CONTRACT_SCHEMAS: Record<string, unknown> = {
  "ApplicationConfig": {
    "additionalProperties": false,
    "description": "サーバーが初期HTMLへ埋め込み、Web UIが起動時に読む設定。\n\nREST応答と同じく、サーバーとWeb UIの間の公開契約である。Web UI側は生成した\nparserで検査してから使用し、解釈できない値を利用者向けエラーとして扱う。",
    "properties": {
      "apiBase": {
        "description": "REST APIの外部prefix。",
        "type": "string"
      },
      "basePath": {
        "description": "画面URLの外部prefix。サブパス配置ではその値になる。",
        "type": "string"
      },
      "path": {
        "description": "prefixを除いた画面内のpath。",
        "type": "string"
      },
      "search": {
        "description": "`?`を含む問い合わせ文字列。無い場合は空文字。",
        "type": "string"
      },
      "styleNonce": {
        "description": "実行時に生成するstyleへ付けるContent Security Policyのnonce。",
        "type": "string"
      }
    },
    "required": [
      "apiBase",
      "basePath",
      "path",
      "search",
      "styleNonce"
    ],
    "type": "object"
  },
  "AttachmentMediaType": {
    "description": "ブラウザーへ能動的な内容として解釈されない、初期対応の静止画像形式。",
    "enum": [
      "image/png",
      "image/jpeg",
      "image/webp"
    ],
    "type": "string"
  },
  "BibliographyImportApplyInput": {
    "additionalProperties": false,
    "properties": {
      "decisions": {
        "items": {
          "$ref": "#/components/schemas/BibliographyImportDecision"
        },
        "maxItems": 1000,
        "minItems": 1,
        "type": "array"
      },
      "items": {
        "items": true,
        "maxItems": 1000,
        "minItems": 1,
        "type": "array"
      },
      "preview_token": {
        "pattern": "^[0-9a-f]{64}$",
        "type": "string"
      },
      "source": {
        "$ref": "#/components/schemas/BibliographyImportSourceInput"
      }
    },
    "required": [
      "source",
      "items",
      "preview_token",
      "decisions"
    ],
    "type": "object"
  },
  "BibliographyImportCandidate": {
    "additionalProperties": false,
    "properties": {
      "citation_key": {
        "type": "string"
      },
      "item_id": {
        "pattern": "^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$",
        "type": "string"
      },
      "matched_by": {
        "items": {
          "type": "string"
        },
        "type": "array"
      },
      "revision": {
        "format": "int64",
        "minimum": 1,
        "type": "integer"
      },
      "title": {
        "type": [
          "string",
          "null"
        ]
      }
    },
    "required": [
      "item_id",
      "citation_key",
      "title",
      "revision",
      "matched_by"
    ],
    "type": "object"
  },
  "BibliographyImportClassification": {
    "enum": [
      "create",
      "update_from_external",
      "unchanged",
      "keep_local",
      "conflict",
      "duplicate_candidate",
      "rejected"
    ],
    "type": "string"
  },
  "BibliographyImportDecision": {
    "additionalProperties": false,
    "properties": {
      "action": {
        "$ref": "#/components/schemas/BibliographyImportDecisionAction"
      },
      "candidate_item_id": {
        "type": [
          "string",
          "null"
        ]
      },
      "position": {
        "format": "uint",
        "minimum": 0,
        "type": "integer"
      }
    },
    "required": [
      "position",
      "action",
      "candidate_item_id"
    ],
    "type": "object"
  },
  "BibliographyImportDecisionAction": {
    "enum": [
      "apply_suggested",
      "create_separate",
      "keep_local",
      "use_external",
      "link_existing_keep_local",
      "link_existing_use_external",
      "exclude"
    ],
    "type": "string"
  },
  "BibliographyImportEntry": {
    "additionalProperties": false,
    "properties": {
      "candidates": {
        "items": {
          "$ref": "#/components/schemas/BibliographyImportCandidate"
        },
        "type": "array"
      },
      "citation_key": {
        "type": [
          "string",
          "null"
        ]
      },
      "classification": {
        "$ref": "#/components/schemas/BibliographyImportClassification"
      },
      "current_csl_json": {
        "type": [
          "object",
          "null"
        ]
      },
      "external_item_id": {
        "type": [
          "string",
          "null"
        ]
      },
      "item_id": {
        "type": [
          "string",
          "null"
        ]
      },
      "item_revision": {
        "format": "int64",
        "minimum": 1,
        "type": [
          "integer",
          "null"
        ]
      },
      "position": {
        "format": "uint",
        "minimum": 0,
        "type": "integer"
      },
      "rejection_code": {
        "type": [
          "string",
          "null"
        ]
      }
    },
    "required": [
      "position",
      "external_item_id",
      "citation_key",
      "classification",
      "item_id",
      "item_revision",
      "current_csl_json",
      "candidates",
      "rejection_code"
    ],
    "type": "object"
  },
  "BibliographyImportPreview": {
    "additionalProperties": false,
    "properties": {
      "entries": {
        "items": {
          "$ref": "#/components/schemas/BibliographyImportEntry"
        },
        "type": "array"
      },
      "preview_token": {
        "pattern": "^[0-9a-f]{64}$",
        "type": "string"
      },
      "source_id": {
        "type": [
          "string",
          "null"
        ]
      },
      "source_revision": {
        "format": "int64",
        "minimum": 1,
        "type": [
          "integer",
          "null"
        ]
      }
    },
    "required": [
      "source_id",
      "source_revision",
      "preview_token",
      "entries"
    ],
    "type": "object"
  },
  "BibliographyImportPreviewInput": {
    "additionalProperties": false,
    "properties": {
      "items": {
        "items": true,
        "maxItems": 1000,
        "minItems": 1,
        "type": "array"
      },
      "source": {
        "$ref": "#/components/schemas/BibliographyImportSourceInput"
      }
    },
    "required": [
      "source",
      "items"
    ],
    "type": "object"
  },
  "BibliographyImportResult": {
    "additionalProperties": false,
    "properties": {
      "created": {
        "format": "uint",
        "minimum": 0,
        "type": "integer"
      },
      "excluded": {
        "format": "uint",
        "minimum": 0,
        "type": "integer"
      },
      "kept": {
        "format": "uint",
        "minimum": 0,
        "type": "integer"
      },
      "source_id": {
        "pattern": "^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$",
        "type": "string"
      },
      "source_revision": {
        "format": "int64",
        "minimum": 1,
        "type": "integer"
      },
      "updated": {
        "format": "uint",
        "minimum": 0,
        "type": "integer"
      }
    },
    "required": [
      "source_id",
      "source_revision",
      "created",
      "updated",
      "kept",
      "excluded"
    ],
    "type": "object"
  },
  "BibliographyImportSource": {
    "additionalProperties": false,
    "properties": {
      "created_at_ms": {
        "format": "int64",
        "type": "integer"
      },
      "display_name": {
        "maxLength": 128,
        "minLength": 1,
        "type": "string"
      },
      "last_imported_at_ms": {
        "format": "int64",
        "type": "integer"
      },
      "method": {
        "type": "string"
      },
      "revision": {
        "format": "int64",
        "minimum": 1,
        "type": "integer"
      },
      "source_id": {
        "pattern": "^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$",
        "type": "string"
      }
    },
    "required": [
      "source_id",
      "method",
      "display_name",
      "revision",
      "created_at_ms",
      "last_imported_at_ms"
    ],
    "type": "object"
  },
  "BibliographyImportSourceInput": {
    "oneOf": [
      {
        "additionalProperties": false,
        "properties": {
          "display_name": {
            "type": "string"
          },
          "kind": {
            "const": "new",
            "type": "string"
          }
        },
        "required": [
          "kind",
          "display_name"
        ],
        "type": "object"
      },
      {
        "additionalProperties": false,
        "properties": {
          "kind": {
            "const": "existing",
            "type": "string"
          },
          "source_id": {
            "type": "string"
          }
        },
        "required": [
          "kind",
          "source_id"
        ],
        "type": "object"
      }
    ]
  },
  "BibliographyItem": {
    "additionalProperties": false,
    "properties": {
      "citation_key": {
        "type": "string"
      },
      "created_at_ms": {
        "format": "int64",
        "type": "integer"
      },
      "csl_json": {
        "type": "object"
      },
      "item_id": {
        "pattern": "^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$",
        "type": "string"
      },
      "revision": {
        "format": "int64",
        "minimum": 1,
        "type": "integer"
      },
      "updated_at_ms": {
        "format": "int64",
        "type": "integer"
      }
    },
    "required": [
      "item_id",
      "citation_key",
      "csl_json",
      "created_at_ms",
      "updated_at_ms",
      "revision"
    ],
    "type": "object"
  },
  "BibliographyItemInput": {
    "additionalProperties": false,
    "properties": {
      "csl_json": {
        "type": "object"
      }
    },
    "required": [
      "csl_json"
    ],
    "type": "object"
  },
  "DeletedNoteListEntry": {
    "additionalProperties": false,
    "properties": {
      "deleted_at_ms": {
        "format": "int64",
        "type": "integer"
      },
      "note_id": {
        "pattern": "^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$",
        "type": "string"
      },
      "purge_at_ms": {
        "format": "int64",
        "type": "integer"
      },
      "revision": {
        "format": "int64",
        "minimum": 1,
        "type": "integer"
      },
      "title": {
        "type": "string"
      }
    },
    "required": [
      "note_id",
      "title",
      "deleted_at_ms",
      "purge_at_ms",
      "revision"
    ],
    "type": "object"
  },
  "DiagnosticSeverity": {
    "enum": [
      "error",
      "warning",
      "information",
      "hint"
    ],
    "type": "string"
  },
  "Health": {
    "additionalProperties": false,
    "properties": {
      "api_version": {
        "type": "string"
      },
      "status": {
        "type": "string"
      }
    },
    "required": [
      "status",
      "api_version"
    ],
    "type": "object"
  },
  "MathMacro": {
    "additionalProperties": false,
    "properties": {
      "argument_count": {
        "format": "uint8",
        "maximum": 9,
        "minimum": 0,
        "type": "integer"
      },
      "name": {
        "maxLength": 32,
        "minLength": 1,
        "pattern": "^[A-Za-z]+$",
        "type": "string"
      },
      "replacement": {
        "maxLength": 512,
        "minLength": 1,
        "type": "string"
      }
    },
    "required": [
      "name",
      "replacement",
      "argument_count"
    ],
    "type": "object"
  },
  "MathMacroSettings": {
    "additionalProperties": false,
    "description": "数式マクロ設定。要求と応答で同じ構造を使う。",
    "properties": {
      "macros": {
        "description": "全項目のコマンド名と置換内容をUTF-8 byte数で合計した上限も拡張属性で公開する。\nJSON配列へ符号化した後の大きさではない。",
        "items": {
          "$ref": "#/components/schemas/MathMacro"
        },
        "maxItems": 64,
        "type": "array",
        "x-marginalis-max-name-replacement-bytes": 16384
      },
      "revision": {
        "format": "int64",
        "minimum": 0,
        "type": "integer"
      }
    },
    "required": [
      "macros",
      "revision"
    ],
    "type": "object"
  },
  "McpClientAuthorization": {
    "additionalProperties": false,
    "properties": {
      "active": {
        "type": "boolean"
      },
      "authorized_at_ms": {
        "format": "int64",
        "type": "integer"
      },
      "client_id": {
        "maxLength": 2048,
        "minLength": 1,
        "type": "string"
      },
      "display_name": {
        "maxLength": 128,
        "minLength": 1,
        "type": "string"
      },
      "granted_scopes": {
        "items": {
          "type": "string"
        },
        "type": "array"
      },
      "last_used_at_ms": {
        "format": "int64",
        "type": [
          "integer",
          "null"
        ]
      },
      "registration_method": {
        "type": "string"
      },
      "scope_ceiling": {
        "items": {
          "type": "string"
        },
        "type": "array"
      },
      "scope_ceiling_configured": {
        "type": "boolean"
      },
      "scope_ceiling_revision": {
        "format": "int64",
        "minimum": 0,
        "type": "integer"
      }
    },
    "required": [
      "client_id",
      "display_name",
      "registration_method",
      "granted_scopes",
      "scope_ceiling_configured",
      "scope_ceiling",
      "scope_ceiling_revision",
      "authorized_at_ms",
      "last_used_at_ms",
      "active"
    ],
    "type": "object"
  },
  "McpScopeCeiling": {
    "additionalProperties": false,
    "properties": {
      "revision": {
        "format": "int64",
        "minimum": 0,
        "type": "integer"
      },
      "scopes": {
        "items": {
          "type": "string"
        },
        "type": "array"
      },
      "supported_scopes": {
        "items": {
          "type": "string"
        },
        "type": "array"
      }
    },
    "required": [
      "supported_scopes",
      "scopes",
      "revision"
    ],
    "type": "object"
  },
  "McpScopeCeilingInput": {
    "additionalProperties": false,
    "properties": {
      "revision": {
        "format": "int64",
        "minimum": 0,
        "type": "integer"
      },
      "scopes": {
        "items": {
          "type": "string"
        },
        "type": "array"
      }
    },
    "required": [
      "scopes",
      "revision"
    ],
    "type": "object"
  },
  "Note": {
    "additionalProperties": false,
    "properties": {
      "created_at_ms": {
        "format": "int64",
        "type": "integer"
      },
      "created_via": {
        "$ref": "#/components/schemas/NoteCreationSource"
      },
      "note_id": {
        "pattern": "^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$",
        "type": "string"
      },
      "review_status": {
        "$ref": "#/components/schemas/NoteReviewStatus"
      },
      "reviewed_at_ms": {
        "format": "int64",
        "type": [
          "integer",
          "null"
        ]
      },
      "reviewed_revision": {
        "format": "int64",
        "minimum": 1,
        "type": [
          "integer",
          "null"
        ]
      },
      "revision": {
        "format": "int64",
        "minimum": 1,
        "type": "integer"
      },
      "source": {
        "type": "string"
      },
      "tags": {
        "items": {
          "type": "string"
        },
        "type": "array"
      },
      "title": {
        "type": "string"
      },
      "updated_at_ms": {
        "format": "int64",
        "type": "integer"
      }
    },
    "required": [
      "note_id",
      "title",
      "source",
      "tags",
      "created_at_ms",
      "updated_at_ms",
      "revision",
      "created_via",
      "review_status",
      "reviewed_revision",
      "reviewed_at_ms"
    ],
    "type": "object"
  },
  "NoteAccess": {
    "description": "ノートに対する実効アクセス水準。大きい水準は小さい水準の操作を含む。\n\nREST、MCP、Web UIで同じ表現を使用する。",
    "enum": [
      "read",
      "edit",
      "manage"
    ],
    "type": "string"
  },
  "NoteAcl": {
    "additionalProperties": false,
    "properties": {
      "entries": {
        "items": {
          "$ref": "#/components/schemas/NoteAclGrant"
        },
        "type": "array"
      }
    },
    "required": [
      "entries"
    ],
    "type": "object"
  },
  "NoteAclEntry": {
    "additionalProperties": false,
    "properties": {
      "issuer": {
        "type": "string"
      },
      "permission": {
        "$ref": "#/components/schemas/NotePermission"
      },
      "subject": {
        "maxLength": 1024,
        "minLength": 1,
        "type": "string"
      }
    },
    "required": [
      "issuer",
      "subject",
      "permission"
    ],
    "type": "object"
  },
  "NoteAclGrant": {
    "additionalProperties": false,
    "properties": {
      "issuer": {
        "type": "string"
      },
      "permission": {
        "$ref": "#/components/schemas/NotePermission"
      },
      "subject": {
        "maxLength": 1024,
        "minLength": 1,
        "type": "string"
      }
    },
    "required": [
      "issuer",
      "subject",
      "permission"
    ],
    "type": "object"
  },
  "NoteAclUpdate": {
    "additionalProperties": false,
    "properties": {
      "entries": {
        "items": {
          "$ref": "#/components/schemas/NoteAclEntry"
        },
        "type": "array"
      }
    },
    "required": [
      "entries"
    ],
    "type": "object"
  },
  "NoteAttachment": {
    "additionalProperties": false,
    "properties": {
      "attachment_id": {
        "pattern": "^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$",
        "type": "string"
      },
      "byte_length": {
        "format": "uint",
        "maximum": 8388608,
        "minimum": 1,
        "type": "integer"
      },
      "created_at_ms": {
        "format": "int64",
        "type": "integer"
      },
      "created_by_issuer": {
        "type": "string"
      },
      "created_by_subject": {
        "type": "string"
      },
      "file_name": {
        "maxLength": 200,
        "minLength": 1,
        "type": "string"
      },
      "media_type": {
        "$ref": "#/components/schemas/AttachmentMediaType"
      },
      "sha256": {
        "pattern": "^[0-9a-f]{64}$",
        "type": "string"
      },
      "source_target": {
        "description": "AsciiDoc本文へ挿入する内部画像target。",
        "type": "string"
      }
    },
    "required": [
      "attachment_id",
      "file_name",
      "media_type",
      "byte_length",
      "sha256",
      "created_at_ms",
      "created_by_issuer",
      "created_by_subject",
      "source_target"
    ],
    "type": "object"
  },
  "NoteCreationSource": {
    "description": "ノートを最初に保存した、サーバー側で判定する接続経路。\n\n作成者の種類、AIの利用、内容の品質を証明する値ではない。",
    "enum": [
      "web",
      "rest",
      "mcp",
      "unknown"
    ],
    "type": "string"
  },
  "NoteDiagnostic": {
    "additionalProperties": false,
    "properties": {
      "code": {
        "type": "string"
      },
      "message": {
        "type": "string"
      },
      "position": {
        "anyOf": [
          {
            "$ref": "#/components/schemas/NoteSourcePosition"
          },
          {
            "type": "null"
          }
        ],
        "description": "本文上の1始まりの表示位置。列はUTF-16 code unit単位で、LSPの既定位置符号化と一致する。"
      },
      "severity": {
        "$ref": "#/components/schemas/DiagnosticSeverity"
      },
      "span": {
        "anyOf": [
          {
            "$ref": "#/components/schemas/Utf8ByteSpan"
          },
          {
            "type": "null"
          }
        ]
      },
      "target": {
        "$ref": "#/components/schemas/NoteValidationTarget"
      }
    },
    "required": [
      "code",
      "severity",
      "target",
      "message"
    ],
    "type": "object"
  },
  "NoteDraft": {
    "additionalProperties": false,
    "properties": {
      "source": {
        "type": "string",
        "x-maxBytes": 524288
      }
    },
    "required": [
      "source"
    ],
    "type": "object"
  },
  "NoteGraph": {
    "additionalProperties": false,
    "description": "グラフビューに出す点と線。\n\n点は現在の利用者が閲覧できるノートと、そのノートが引用している文献だけを含む。線は始点と\n終点の両方が点として含まれる場合だけ返す。閲覧できないノートの存在も件数も現れない。",
    "properties": {
      "citations": {
        "items": {
          "$ref": "#/components/schemas/NoteGraphCitation"
        },
        "type": "array"
      },
      "notes": {
        "items": {
          "$ref": "#/components/schemas/NoteGraphNote"
        },
        "type": "array"
      },
      "references": {
        "items": {
          "$ref": "#/components/schemas/NoteGraphReference"
        },
        "type": "array"
      },
      "works": {
        "items": {
          "$ref": "#/components/schemas/NoteGraphWork"
        },
        "type": "array"
      }
    },
    "required": [
      "notes",
      "works",
      "references",
      "citations"
    ],
    "type": "object"
  },
  "NoteGraphCitation": {
    "additionalProperties": false,
    "description": "ノートから文献への引用。",
    "properties": {
      "citation_key": {
        "type": "string"
      },
      "source_note_id": {
        "pattern": "^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$",
        "type": "string"
      }
    },
    "required": [
      "source_note_id",
      "citation_key"
    ],
    "type": "object"
  },
  "NoteGraphNote": {
    "additionalProperties": false,
    "description": "図に出すノート。本文は含まない。",
    "properties": {
      "note_id": {
        "pattern": "^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$",
        "type": "string"
      },
      "tags": {
        "items": {
          "type": "string"
        },
        "type": "array"
      },
      "title": {
        "type": "string"
      },
      "updated_at_ms": {
        "format": "int64",
        "type": "integer"
      }
    },
    "required": [
      "note_id",
      "title",
      "tags",
      "updated_at_ms"
    ],
    "type": "object"
  },
  "NoteGraphReference": {
    "additionalProperties": false,
    "description": "ノートからノートへの参照。",
    "properties": {
      "source_note_id": {
        "pattern": "^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$",
        "type": "string"
      },
      "target_note_id": {
        "pattern": "^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$",
        "type": "string"
      }
    },
    "required": [
      "source_note_id",
      "target_note_id"
    ],
    "type": "object"
  },
  "NoteGraphWork": {
    "additionalProperties": false,
    "description": "図に出す文献。文献情報そのものではなく、引用されたという事実を表す。",
    "properties": {
      "citation_key": {
        "type": "string"
      },
      "title": {
        "description": "引用元のノートを書いた利用者のライブラリで解決できた場合の題名。",
        "type": [
          "string",
          "null"
        ]
      }
    },
    "required": [
      "citation_key",
      "title"
    ],
    "type": "object"
  },
  "NoteListEntry": {
    "description": "一覧の1項目。ノート要約に実効アクセス水準を加えたもの。",
    "properties": {
      "access": {
        "$ref": "#/components/schemas/NoteAccess"
      },
      "created_via": {
        "$ref": "#/components/schemas/NoteCreationSource"
      },
      "note_id": {
        "pattern": "^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$",
        "type": "string"
      },
      "review_status": {
        "$ref": "#/components/schemas/NoteReviewStatus"
      },
      "reviewed_at_ms": {
        "format": "int64",
        "type": [
          "integer",
          "null"
        ]
      },
      "reviewed_revision": {
        "format": "int64",
        "minimum": 1,
        "type": [
          "integer",
          "null"
        ]
      },
      "revision": {
        "format": "int64",
        "minimum": 1,
        "type": "integer"
      },
      "tags": {
        "items": {
          "type": "string"
        },
        "type": "array"
      },
      "title": {
        "type": "string"
      },
      "updated_at_ms": {
        "format": "int64",
        "type": "integer"
      }
    },
    "required": [
      "note_id",
      "title",
      "tags",
      "updated_at_ms",
      "revision",
      "created_via",
      "review_status",
      "reviewed_revision",
      "reviewed_at_ms",
      "access"
    ],
    "type": "object"
  },
  "NotePermission": {
    "description": "ACLで共有先へ与える権限。REST、MCP、archiveで同じ表現を使用する。",
    "enum": [
      "read",
      "edit"
    ],
    "type": "string"
  },
  "NotePreview": {
    "additionalProperties": false,
    "properties": {
      "diagnostics": {
        "items": {
          "$ref": "#/components/schemas/NoteDiagnostic"
        },
        "type": "array"
      },
      "html": {
        "type": "string"
      },
      "math_macros": {
        "items": {
          "$ref": "#/components/schemas/MathMacro"
        },
        "type": "array"
      },
      "spans": {
        "description": "編集画面の装飾に使うspan注釈。原文の出現順で、同じ開始位置では外側が先になる。",
        "items": {
          "$ref": "#/components/schemas/NoteSourceSpan"
        },
        "type": "array"
      }
    },
    "required": [
      "html",
      "diagnostics",
      "math_macros",
      "spans"
    ],
    "type": "object"
  },
  "NoteReview": {
    "additionalProperties": false,
    "properties": {
      "current_revision": {
        "format": "int64",
        "minimum": 1,
        "type": "integer"
      },
      "note_id": {
        "pattern": "^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$",
        "type": "string"
      },
      "reviewed_at_ms": {
        "format": "int64",
        "type": [
          "integer",
          "null"
        ]
      },
      "reviewed_revision": {
        "format": "int64",
        "minimum": 1,
        "type": [
          "integer",
          "null"
        ]
      },
      "reviewer_issuer": {
        "type": [
          "string",
          "null"
        ]
      },
      "reviewer_subject": {
        "type": [
          "string",
          "null"
        ]
      },
      "status": {
        "$ref": "#/components/schemas/NoteReviewStatus"
      }
    },
    "required": [
      "note_id",
      "current_revision",
      "status",
      "reviewed_revision",
      "reviewed_at_ms",
      "reviewer_issuer",
      "reviewer_subject"
    ],
    "type": "object"
  },
  "NoteReviewStatus": {
    "description": "現在のrevisionに対する人手確認状態。",
    "enum": [
      "unknown",
      "pending",
      "reviewed"
    ],
    "type": "string"
  },
  "NoteRevision": {
    "additionalProperties": false,
    "description": "一つのrevisionが確定した直後のノート状態。",
    "properties": {
      "access": {
        "$ref": "#/components/schemas/NoteAccess"
      },
      "changed_by_issuer": {
        "type": "string"
      },
      "changed_by_subject": {
        "type": "string"
      },
      "deleted_at_ms": {
        "format": "int64",
        "type": [
          "integer",
          "null"
        ]
      },
      "kind": {
        "$ref": "#/components/schemas/NoteRevisionKind"
      },
      "note": {
        "$ref": "#/components/schemas/Note"
      }
    },
    "required": [
      "note",
      "access",
      "deleted_at_ms",
      "changed_by_issuer",
      "changed_by_subject",
      "kind"
    ],
    "type": "object"
  },
  "NoteRevisionDiff": {
    "additionalProperties": false,
    "description": "二つのrevisionのAsciiDoc原文から要求時に生成した行単位の差分。",
    "properties": {
      "from_revision": {
        "format": "int64",
        "minimum": 1,
        "type": "integer"
      },
      "to_revision": {
        "format": "int64",
        "minimum": 1,
        "type": "integer"
      },
      "unified_diff": {
        "type": "string"
      }
    },
    "required": [
      "from_revision",
      "to_revision",
      "unified_diff"
    ],
    "type": "object"
  },
  "NoteRevisionKind": {
    "description": "ノートのrevisionを生じさせた操作。\n\n本文に差がないACL変更や確認操作も、競合検出では同じrevisionを進めるため履歴へ残す。",
    "oneOf": [
      {
        "enum": [
          "created",
          "content_updated",
          "acl_updated",
          "reviewed",
          "deleted",
          "restored",
          "history_restored"
        ],
        "type": "string"
      },
      {
        "const": "imported",
        "description": "履歴導入前の現行版または旧archiveから作った基準点。",
        "type": "string"
      }
    ]
  },
  "NoteRevisionSummary": {
    "additionalProperties": false,
    "description": "本文を含まない一つのrevision情報。",
    "properties": {
      "changed_at_ms": {
        "format": "int64",
        "type": "integer"
      },
      "changed_by_issuer": {
        "type": "string"
      },
      "changed_by_subject": {
        "type": "string"
      },
      "kind": {
        "$ref": "#/components/schemas/NoteRevisionKind"
      },
      "revision": {
        "format": "int64",
        "minimum": 1,
        "type": "integer"
      }
    },
    "required": [
      "revision",
      "changed_at_ms",
      "changed_by_issuer",
      "changed_by_subject",
      "kind"
    ],
    "type": "object"
  },
  "NoteSourcePosition": {
    "additionalProperties": false,
    "properties": {
      "column": {
        "format": "uint32",
        "minimum": 1,
        "type": "integer"
      },
      "line": {
        "format": "uint32",
        "minimum": 1,
        "type": "integer"
      }
    },
    "required": [
      "line",
      "column"
    ],
    "type": "object"
  },
  "NoteSourceSpan": {
    "additionalProperties": false,
    "description": "編集画面の装飾に使う、本文中の記法1件の位置。\n\n範囲は原文のUTF-8バイトオフセットで、診断のspanと同じ数え方を使う。",
    "properties": {
      "content_span": {
        "anyOf": [
          {
            "$ref": "#/components/schemas/Utf8ByteSpan"
          },
          {
            "type": "null"
          }
        ],
        "description": "記法文字を除いた、装飾対象の本文部分。区別を持たない記法では省略する。"
      },
      "kind": {
        "$ref": "#/components/schemas/NoteSourceSpanKind"
      },
      "level": {
        "description": "見出しの深さ。`==`が1で、文書題名を除く。見出し以外は省略する。",
        "format": "uint8",
        "maximum": 255,
        "minimum": 0,
        "type": [
          "integer",
          "null"
        ]
      },
      "marker_spans": {
        "description": "カーソルが離れているときに折り畳める記法文字の範囲。",
        "items": {
          "$ref": "#/components/schemas/Utf8ByteSpan"
        },
        "type": "array"
      },
      "span": {
        "$ref": "#/components/schemas/Utf8ByteSpan",
        "description": "記法全体が占める範囲。"
      }
    },
    "required": [
      "kind",
      "span"
    ],
    "type": "object"
  },
  "NoteSourceSpanKind": {
    "description": "span注釈が区別する記法の種類。",
    "enum": [
      "document_title",
      "heading",
      "document_attribute",
      "anchor",
      "strong",
      "emphasis",
      "highlight",
      "subscript",
      "superscript",
      "monospace",
      "link",
      "cross_reference",
      "citation",
      "inline_math",
      "math_block",
      "source_block",
      "literal_block",
      "quote",
      "example",
      "admonition",
      "table",
      "list_item"
    ],
    "type": "string"
  },
  "NoteSummary": {
    "additionalProperties": false,
    "properties": {
      "created_via": {
        "$ref": "#/components/schemas/NoteCreationSource"
      },
      "note_id": {
        "pattern": "^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$",
        "type": "string"
      },
      "review_status": {
        "$ref": "#/components/schemas/NoteReviewStatus"
      },
      "reviewed_at_ms": {
        "format": "int64",
        "type": [
          "integer",
          "null"
        ]
      },
      "reviewed_revision": {
        "format": "int64",
        "minimum": 1,
        "type": [
          "integer",
          "null"
        ]
      },
      "revision": {
        "format": "int64",
        "minimum": 1,
        "type": "integer"
      },
      "tags": {
        "items": {
          "type": "string"
        },
        "type": "array"
      },
      "title": {
        "type": "string"
      },
      "updated_at_ms": {
        "format": "int64",
        "type": "integer"
      }
    },
    "required": [
      "note_id",
      "title",
      "tags",
      "updated_at_ms",
      "revision",
      "created_via",
      "review_status",
      "reviewed_revision",
      "reviewed_at_ms"
    ],
    "type": "object"
  },
  "NoteValidationTarget": {
    "description": "入力上の問題が、ノート入力のどの部分にあるかを示す位置。\n\nREST、MCP、Web UIで同じ表現を使用する。`field`を判別子とし、`tag`と`acl_entry`は\n対象の添字を伴う。",
    "oneOf": [
      {
        "additionalProperties": false,
        "properties": {
          "field": {
            "const": "source",
            "type": "string"
          }
        },
        "required": [
          "field"
        ],
        "type": "object"
      },
      {
        "additionalProperties": false,
        "properties": {
          "field": {
            "const": "title",
            "type": "string"
          }
        },
        "required": [
          "field"
        ],
        "type": "object"
      },
      {
        "additionalProperties": false,
        "properties": {
          "field": {
            "const": "body",
            "type": "string"
          }
        },
        "required": [
          "field"
        ],
        "type": "object"
      },
      {
        "additionalProperties": false,
        "properties": {
          "field": {
            "const": "tag",
            "type": "string"
          },
          "index": {
            "format": "uint",
            "minimum": 0,
            "type": "integer"
          }
        },
        "required": [
          "field",
          "index"
        ],
        "type": "object"
      },
      {
        "additionalProperties": false,
        "properties": {
          "field": {
            "const": "tags",
            "type": "string"
          }
        },
        "required": [
          "field"
        ],
        "type": "object"
      },
      {
        "additionalProperties": false,
        "properties": {
          "field": {
            "const": "acl_entry",
            "type": "string"
          },
          "index": {
            "format": "uint",
            "minimum": 0,
            "type": "integer"
          }
        },
        "required": [
          "field",
          "index"
        ],
        "type": "object"
      }
    ]
  },
  "NoteView": {
    "additionalProperties": false,
    "properties": {
      "access": {
        "$ref": "#/components/schemas/NoteAccess"
      },
      "html": {
        "type": "string"
      },
      "math_macros": {
        "items": {
          "$ref": "#/components/schemas/MathMacro"
        },
        "type": "array"
      },
      "note": {
        "$ref": "#/components/schemas/Note"
      },
      "related": {
        "$ref": "#/components/schemas/RelatedNotes"
      }
    },
    "required": [
      "note",
      "access",
      "html",
      "related",
      "math_macros"
    ],
    "type": "object"
  },
  "Problem": {
    "additionalProperties": false,
    "properties": {
      "code": {
        "$ref": "#/components/schemas/ProblemCode"
      },
      "diagnostics": {
        "items": {
          "$ref": "#/components/schemas/NoteDiagnostic"
        },
        "type": "array"
      },
      "message": {
        "type": "string"
      }
    },
    "required": [
      "code",
      "message"
    ],
    "type": "object"
  },
  "ProblemCode": {
    "enum": [
      "authentication_required",
      "authentication_unavailable",
      "csrf_rejected",
      "csrf_required",
      "csrf_invalid",
      "same_origin_required",
      "origin_not_allowed",
      "not_found",
      "forbidden",
      "conflict",
      "retention_expired",
      "invalid_sync_cursor",
      "sync_cursor_expired",
      "precondition_required",
      "invalid_request",
      "patch_rejected",
      "validation_failed",
      "advisories_rejected",
      "render_failed",
      "unavailable"
    ],
    "type": "string"
  },
  "RelatedNotes": {
    "additionalProperties": false,
    "properties": {
      "incoming": {
        "items": {
          "$ref": "#/components/schemas/NoteSummary"
        },
        "type": "array"
      },
      "outgoing": {
        "items": {
          "$ref": "#/components/schemas/NoteSummary"
        },
        "type": "array"
      }
    },
    "required": [
      "outgoing",
      "incoming"
    ],
    "type": "object"
  },
  "Session": {
    "additionalProperties": false,
    "properties": {
      "issuer": {
        "type": "string"
      },
      "subject": {
        "type": "string"
      }
    },
    "required": [
      "issuer",
      "subject"
    ],
    "type": "object"
  },
  "Utf8ByteSpan": {
    "additionalProperties": false,
    "properties": {
      "end": {
        "format": "uint32",
        "minimum": 0,
        "type": "integer"
      },
      "start": {
        "format": "uint32",
        "minimum": 0,
        "type": "integer"
      },
      "unit": {
        "$ref": "#/components/schemas/Utf8ByteUnit"
      }
    },
    "required": [
      "start",
      "end",
      "unit"
    ],
    "type": "object"
  },
  "Utf8ByteUnit": {
    "enum": [
      "utf8_byte"
    ],
    "type": "string"
  },
  "WebhookEventKind": {
    "description": "Webhookが通知するeventの種別。",
    "enum": [
      "note.created",
      "note.updated",
      "note.deleted",
      "note.restored",
      "bibliography_item.created",
      "bibliography_item.updated",
      "bibliography_item.deleted"
    ],
    "type": "string"
  },
  "WebhookSecret": {
    "additionalProperties": false,
    "properties": {
      "secret": {
        "minLength": 1,
        "type": "string"
      }
    },
    "required": [
      "secret"
    ],
    "type": "object"
  },
  "WebhookSubscription": {
    "additionalProperties": false,
    "properties": {
      "created_at_ms": {
        "format": "int64",
        "type": "integer"
      },
      "disabled_reason": {
        "enum": [
          "delivery_exhausted",
          "destination_rejected",
          "owner_disabled",
          null
        ],
        "type": [
          "string",
          "null"
        ]
      },
      "event_kinds": {
        "items": {
          "$ref": "#/components/schemas/WebhookEventKind"
        },
        "type": "array"
      },
      "last_attempted_at_ms": {
        "format": "int64",
        "type": [
          "integer",
          "null"
        ]
      },
      "last_failure": {
        "enum": [
          "non_success_status",
          "connect_failed",
          "timed_out",
          "destination_rejected",
          null
        ],
        "type": [
          "string",
          "null"
        ]
      },
      "next_attempt_at_ms": {
        "format": "int64",
        "type": [
          "integer",
          "null"
        ]
      },
      "pending_count": {
        "format": "int64",
        "minimum": 0,
        "type": "integer"
      },
      "revision": {
        "format": "int64",
        "minimum": 0,
        "type": "integer"
      },
      "state": {
        "$ref": "#/components/schemas/WebhookSubscriptionState"
      },
      "subscription_id": {
        "pattern": "^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$",
        "type": "string"
      },
      "updated_at_ms": {
        "format": "int64",
        "type": "integer"
      },
      "url": {
        "type": "string"
      }
    },
    "required": [
      "subscription_id",
      "url",
      "event_kinds",
      "state",
      "disabled_reason",
      "created_at_ms",
      "updated_at_ms",
      "revision",
      "last_attempted_at_ms",
      "last_failure",
      "next_attempt_at_ms",
      "pending_count"
    ],
    "type": "object"
  },
  "WebhookSubscriptionCreated": {
    "additionalProperties": false,
    "description": "登録直後の応答。secretはこの応答とsecret再生成の応答でだけ返す。",
    "properties": {
      "secret": {
        "minLength": 1,
        "type": "string"
      },
      "subscription": {
        "$ref": "#/components/schemas/WebhookSubscription"
      }
    },
    "required": [
      "subscription",
      "secret"
    ],
    "type": "object"
  },
  "WebhookSubscriptionDraft": {
    "additionalProperties": false,
    "properties": {
      "event_kinds": {
        "items": {
          "$ref": "#/components/schemas/WebhookEventKind"
        },
        "minItems": 1,
        "type": "array"
      },
      "url": {
        "description": "通知の送信先URL。公開networkのHTTPS(port 443)だけを受け付ける。",
        "maxLength": 2048,
        "minLength": 1,
        "type": "string"
      }
    },
    "required": [
      "url",
      "event_kinds"
    ],
    "type": "object"
  },
  "WebhookSubscriptionState": {
    "enum": [
      "pending_challenge",
      "active",
      "disabled"
    ],
    "type": "string"
  },
  "WebhookVerification": {
    "additionalProperties": false,
    "description": "所有確認の結果。失敗しても購読は残り、やり直せる。",
    "properties": {
      "failure": {
        "enum": [
          "non_success_status",
          "connect_failed",
          "timed_out",
          "destination_rejected",
          null
        ],
        "type": [
          "string",
          "null"
        ]
      },
      "verified": {
        "type": "boolean"
      }
    },
    "required": [
      "verified",
      "failure"
    ],
    "type": "object"
  }
};

function resolveSchema(schema: unknown): unknown {
  if (
    typeof schema === "object" &&
    schema !== null &&
    "$ref" in schema &&
    typeof (schema as { $ref: unknown }).$ref === "string"
  ) {
    const name = (schema as { $ref: string }).$ref.split("/").pop() ?? "";
    const target = CONTRACT_SCHEMAS[name];
    if (target === undefined) throw new Error(`schema ${name} is missing`);
    return resolveSchema(target);
  }
  return schema;
}

function matchesType(value: unknown, type: string): boolean {
  switch (type) {
    case "object":
      return typeof value === "object" && value !== null && !Array.isArray(value);
    case "array":
      return Array.isArray(value);
    case "string":
      return typeof value === "string";
    case "integer":
      return Number.isSafeInteger(value);
    case "number":
      return typeof value === "number" && Number.isFinite(value);
    case "boolean":
      return typeof value === "boolean";
    case "null":
      return value === null;
    default:
      return true;
  }
}

function isValid(value: unknown, schema: unknown): boolean {
  try {
    assertValid(value, schema, "value");
    return true;
  } catch {
    return false;
  }
}

function assertValid(value: unknown, rawSchema: unknown, path: string): void {
  const schema = resolveSchema(rawSchema);
  if (schema === true || schema === undefined) return;
  if (schema === false) throw new Error(`${path} is invalid`);
  if (typeof schema !== "object" || schema === null) return;
  const s = schema as Record<string, unknown>;
  if (Array.isArray(s.allOf)) {
    for (const member of s.allOf) assertValid(value, member, path);
  }
  const alternatives = (s.oneOf ?? s.anyOf) as unknown[] | undefined;
  if (Array.isArray(alternatives)) {
    if (!alternatives.some((member) => isValid(value, member))) {
      throw new Error(`${path} is invalid`);
    }
  }
  if (s.type !== undefined) {
    const types = Array.isArray(s.type) ? s.type : [s.type];
    if (!types.some((type) => typeof type === "string" && matchesType(value, type))) {
      throw new Error(`${path} is invalid`);
    }
  }
  if (s.const !== undefined && JSON.stringify(value) !== JSON.stringify(s.const)) {
    throw new Error(`${path} is invalid`);
  }
  if (Array.isArray(s.enum)) {
    if (!s.enum.some((member) => JSON.stringify(member) === JSON.stringify(value))) {
      throw new Error(`${path} is invalid`);
    }
  }
  if (typeof value === "string") {
    // JSON Schemaの文字列長はUnicode code point数であり、JavaScriptのUTF-16 code unit数ではない。
    const characterLength = Array.from(value).length;
    if (typeof s.minLength === "number" && characterLength < s.minLength) {
      throw new Error(`${path} is invalid`);
    }
    if (typeof s.maxLength === "number" && characterLength > s.maxLength) {
      throw new Error(`${path} is invalid`);
    }
    if (typeof s.pattern === "string" && !new RegExp(s.pattern).test(value)) {
      throw new Error(`${path} is invalid`);
    }
  }
  if (typeof value === "number") {
    if (typeof s.minimum === "number" && value < s.minimum) {
      throw new Error(`${path} is invalid`);
    }
    if (typeof s.maximum === "number" && value > s.maximum) {
      throw new Error(`${path} is invalid`);
    }
  }
  if (Array.isArray(value)) {
    if (typeof s.minItems === "number" && value.length < s.minItems) {
      throw new Error(`${path} is invalid`);
    }
    if (typeof s.maxItems === "number" && value.length > s.maxItems) {
      throw new Error(`${path} is invalid`);
    }
    if (s.items !== undefined) {
      value.forEach((item, index) => assertValid(item, s.items, `${path}[${index}]`));
    }
  }
  if (typeof value === "object" && value !== null && !Array.isArray(value)) {
    const record = value as Record<string, unknown>;
    const properties = (s.properties ?? {}) as Record<string, unknown>;
    if (Array.isArray(s.required)) {
      for (const name of s.required) {
        if (typeof name === "string" && record[name] === undefined) {
          throw new Error(`${path}.${name} is missing`);
        }
      }
    }
    for (const [name, property] of Object.entries(properties)) {
      if (record[name] !== undefined) {
        assertValid(record[name], property, `${path}.${name}`);
      }
    }
    if (s.additionalProperties === false && s.properties !== undefined) {
      for (const name of Object.keys(record)) {
        if (!(name in properties)) {
          throw new Error(`${path}.${name} is not allowed`);
        }
      }
    }
  }
}

function parseAs<T>(value: unknown, schemaName: string, label: string): T {
  assertValid(value, CONTRACT_SCHEMAS[schemaName], label);
  return value as T;
}

function parseArrayAs<T>(value: unknown, schemaName: string, label: string): T[] {
  if (!Array.isArray(value)) throw new Error(`${label} are invalid`);
  return value.map((item, index) => parseAs<T>(item, schemaName, `${label}[${index}]`));
}
export function parseApplicationConfig(value: unknown): ApplicationConfig {
  return parseAs<ApplicationConfig>(value, "ApplicationConfig", "application config");
}
export function parseNote(value: unknown): Note {
  return parseAs<Note>(value, "Note", "note");
}
export function parseNoteSummary(value: unknown): NoteSummary {
  return parseAs<NoteSummary>(value, "NoteSummary", "note summary");
}
export function parseNoteSummaries(value: unknown): NoteSummary[] {
  return parseArrayAs<NoteSummary>(value, "NoteSummary", "note summaries");
}
export function parseNoteListEntry(value: unknown): NoteListEntry {
  return parseAs<NoteListEntry>(value, "NoteListEntry", "note list entry");
}
export function parseNoteListEntries(value: unknown): NoteListEntry[] {
  return parseArrayAs<NoteListEntry>(value, "NoteListEntry", "note list entries");
}
export function parseDeletedNoteListEntries(value: unknown): DeletedNoteListEntry[] {
  return parseArrayAs<DeletedNoteListEntry>(value, "DeletedNoteListEntry", "deleted note list entries");
}
export function parseNoteReview(value: unknown): NoteReview {
  return parseAs<NoteReview>(value, "NoteReview", "note review");
}
export function parseNoteRevisionSummaries(value: unknown): NoteRevisionSummary[] {
  return parseArrayAs<NoteRevisionSummary>(value, "NoteRevisionSummary", "note revision summaries");
}
export function parseNoteRevision(value: unknown): NoteRevision {
  return parseAs<NoteRevision>(value, "NoteRevision", "note revision");
}
export function parseNoteRevisionDiff(value: unknown): NoteRevisionDiff {
  return parseAs<NoteRevisionDiff>(value, "NoteRevisionDiff", "note revision diff");
}
export function parseNoteAttachment(value: unknown): NoteAttachment {
  return parseAs<NoteAttachment>(value, "NoteAttachment", "note attachment");
}
export function parseNoteAttachments(value: unknown): NoteAttachment[] {
  return parseArrayAs<NoteAttachment>(value, "NoteAttachment", "note attachments");
}
export function parseNoteGraph(value: unknown): NoteGraph {
  return parseAs<NoteGraph>(value, "NoteGraph", "note graph");
}
export function parseNoteView(value: unknown): NoteView {
  return parseAs<NoteView>(value, "NoteView", "note view");
}
export function parseNoteAcl(value: unknown): NoteAcl {
  return parseAs<NoteAcl>(value, "NoteAcl", "note ACL");
}
export function parseNotePreview(value: unknown): NotePreview {
  return parseAs<NotePreview>(value, "NotePreview", "note preview");
}
export function parseProblem(value: unknown): Problem {
  return parseAs<Problem>(value, "Problem", "problem");
}
export function parseMathMacroSettings(value: unknown): MathMacroSettings {
  return parseAs<MathMacroSettings>(value, "MathMacroSettings", "math macro settings");
}
export function parseMcpScopeCeiling(value: unknown): McpScopeCeiling {
  return parseAs<McpScopeCeiling>(value, "McpScopeCeiling", "MCP scope ceiling");
}
export function parseMcpClientAuthorization(value: unknown): McpClientAuthorization {
  return parseAs<McpClientAuthorization>(value, "McpClientAuthorization", "MCP client authorization");
}
export function parseMcpClientAuthorizations(value: unknown): McpClientAuthorization[] {
  return parseArrayAs<McpClientAuthorization>(value, "McpClientAuthorization", "MCP client authorizations");
}
export function parseBibliographyItem(value: unknown): BibliographyItem {
  return parseAs<BibliographyItem>(value, "BibliographyItem", "bibliography item");
}
export function parseBibliographyItems(value: unknown): BibliographyItem[] {
  return parseArrayAs<BibliographyItem>(value, "BibliographyItem", "bibliography items");
}
export function parseBibliographyImportSources(value: unknown): BibliographyImportSource[] {
  return parseArrayAs<BibliographyImportSource>(value, "BibliographyImportSource", "bibliography import sources");
}
export function parseBibliographyImportPreview(value: unknown): BibliographyImportPreview {
  return parseAs<BibliographyImportPreview>(value, "BibliographyImportPreview", "bibliography import preview");
}
export function parseBibliographyImportResult(value: unknown): BibliographyImportResult {
  return parseAs<BibliographyImportResult>(value, "BibliographyImportResult", "bibliography import result");
}
export function parseWebhookSubscriptions(value: unknown): WebhookSubscription[] {
  return parseArrayAs<WebhookSubscription>(value, "WebhookSubscription", "webhook subscriptions");
}
export function parseWebhookSubscriptionCreated(value: unknown): WebhookSubscriptionCreated {
  return parseAs<WebhookSubscriptionCreated>(value, "WebhookSubscriptionCreated", "created webhook subscription");
}
export function parseWebhookSecret(value: unknown): WebhookSecret {
  return parseAs<WebhookSecret>(value, "WebhookSecret", "webhook secret");
}
export function parseWebhookVerification(value: unknown): WebhookVerification {
  return parseAs<WebhookVerification>(value, "WebhookVerification", "webhook verification");
}
