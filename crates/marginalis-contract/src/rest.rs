//! REST APIとTypeScriptクライアントで共有する公開契約。
//!
//! 権限、アクセス水準、検証対象など、公開表現が業務モデルと同一である値は
//! [`marginalis_domain`]の定義を参照する。要求と応答の構造だけをこのmoduleで定義する。

use marginalis_domain::{
    ENTITY_ID_PATTERN, MAX_GRAPH_DEPTH, NOTE_POLICY, NoteAccess, NoteCreationSource,
    NotePermission, NoteReviewStatus, NoteValidationTarget, Revision,
};
use schemars::{JsonSchema, SchemaGenerator, generate::SchemaSettings};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub const API_VERSION: &str = "v3";
pub const API_PREFIX: &str = "/api/v3";

/// ノートと書誌のrevisionが取り得る最小値。schemars属性から参照する。
const MINIMUM_REVISION: i64 = Revision::MINIMUM_VALUE;

/// `#[schemars(required)]`を付けたOption fieldへ、nullを許す型を戻すtransform。
///
/// schemarsの`required`は内側の型のschemaを使うため、そのままではnullが表現から消える。
/// 応答は「必須だがnullになりうる」項目としてこの2つを組み合わせる。
pub(crate) fn nullable(schema: &mut schemars::Schema) {
    if let Some(object) = schema.as_object_mut() {
        match object.get_mut("type") {
            Some(Value::String(single)) => {
                let single = std::mem::take(single);
                object.insert(
                    "type".into(),
                    Value::Array(vec![Value::String(single), Value::String("null".into())]),
                );
            }
            Some(Value::Array(types)) if !types.iter().any(|kind| kind == "null") => {
                types.push(Value::String("null".into()));
            }
            _ => {}
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RestRouteContract {
    pub method: &'static str,
    pub specification_path: &'static str,
    pub probe_path: &'static str,
}

pub const REST_ROUTE_CONTRACTS: &[RestRouteContract] = &[
    RestRouteContract {
        method: "GET",
        specification_path: "/api/v3/health",
        probe_path: "/api/v3/health",
    },
    RestRouteContract {
        method: "GET",
        specification_path: "/api/v3/session",
        probe_path: "/api/v3/session",
    },
    RestRouteContract {
        method: "GET",
        specification_path: "/api/v3/math-macros",
        probe_path: "/api/v3/math-macros",
    },
    RestRouteContract {
        method: "PUT",
        specification_path: "/api/v3/math-macros",
        probe_path: "/api/v3/math-macros",
    },
    RestRouteContract {
        method: "GET",
        specification_path: "/api/v3/mcp-scope-ceilings",
        probe_path: "/api/v3/mcp-scope-ceilings",
    },
    RestRouteContract {
        method: "PUT",
        specification_path: "/api/v3/mcp-scope-ceilings",
        probe_path: "/api/v3/mcp-scope-ceilings",
    },
    RestRouteContract {
        method: "GET",
        specification_path: "/api/v3/mcp-authorizations",
        probe_path: "/api/v3/mcp-authorizations",
    },
    RestRouteContract {
        method: "PUT",
        specification_path: "/api/v3/mcp-authorizations/{client_id}/scope-ceiling",
        probe_path: "/api/v3/mcp-authorizations/mcp-client/scope-ceiling",
    },
    RestRouteContract {
        method: "GET",
        specification_path: "/api/v3/notes",
        probe_path: "/api/v3/notes",
    },
    RestRouteContract {
        method: "GET",
        specification_path: "/api/v3/notes/deleted",
        probe_path: "/api/v3/notes/deleted",
    },
    RestRouteContract {
        method: "GET",
        specification_path: "/api/v3/bibliography",
        probe_path: "/api/v3/bibliography",
    },
    RestRouteContract {
        method: "POST",
        specification_path: "/api/v3/bibliography",
        probe_path: "/api/v3/bibliography",
    },
    RestRouteContract {
        method: "PUT",
        specification_path: "/api/v3/bibliography/{item_id}",
        probe_path: "/api/v3/bibliography/0197c9bc-0000-7000-8000-000000000001",
    },
    RestRouteContract {
        method: "DELETE",
        specification_path: "/api/v3/bibliography/{item_id}",
        probe_path: "/api/v3/bibliography/0197c9bc-0000-7000-8000-000000000001",
    },
    RestRouteContract {
        method: "GET",
        specification_path: "/api/v3/bibliography/import-sources",
        probe_path: "/api/v3/bibliography/import-sources",
    },
    RestRouteContract {
        method: "POST",
        specification_path: "/api/v3/bibliography/import-previews",
        probe_path: "/api/v3/bibliography/import-previews",
    },
    RestRouteContract {
        method: "POST",
        specification_path: "/api/v3/bibliography/imports",
        probe_path: "/api/v3/bibliography/imports",
    },
    RestRouteContract {
        method: "POST",
        specification_path: "/api/v3/notes",
        probe_path: "/api/v3/notes",
    },
    RestRouteContract {
        method: "POST",
        specification_path: "/api/v3/web/notes",
        probe_path: "/api/v3/web/notes",
    },
    RestRouteContract {
        method: "POST",
        specification_path: "/api/v3/notes/preview",
        probe_path: "/api/v3/notes/preview",
    },
    RestRouteContract {
        method: "POST",
        specification_path: "/api/v3/notes/{note_id}/preview",
        probe_path: "/api/v3/notes/0197c9bc-0000-7000-8000-000000000001/preview",
    },
    RestRouteContract {
        method: "GET",
        specification_path: "/api/v3/notes/{note_id}",
        probe_path: "/api/v3/notes/0197c9bc-0000-7000-8000-000000000001",
    },
    RestRouteContract {
        method: "PUT",
        specification_path: "/api/v3/notes/{note_id}",
        probe_path: "/api/v3/notes/0197c9bc-0000-7000-8000-000000000001",
    },
    RestRouteContract {
        method: "DELETE",
        specification_path: "/api/v3/notes/{note_id}",
        probe_path: "/api/v3/notes/0197c9bc-0000-7000-8000-000000000001",
    },
    RestRouteContract {
        method: "GET",
        specification_path: "/api/v3/notes/{note_id}/review",
        probe_path: "/api/v3/notes/0197c9bc-0000-7000-8000-000000000001/review",
    },
    RestRouteContract {
        method: "POST",
        specification_path: "/api/v3/notes/{note_id}/review",
        probe_path: "/api/v3/notes/0197c9bc-0000-7000-8000-000000000001/review",
    },
    RestRouteContract {
        method: "GET",
        specification_path: "/api/v3/notes/graph",
        probe_path: "/api/v3/notes/graph",
    },
    RestRouteContract {
        method: "GET",
        specification_path: "/api/v3/notes/{note_id}/view",
        probe_path: "/api/v3/notes/0197c9bc-0000-7000-8000-000000000001/view",
    },
    RestRouteContract {
        method: "POST",
        specification_path: "/api/v3/notes/{note_id}/restore",
        probe_path: "/api/v3/notes/0197c9bc-0000-7000-8000-000000000001/restore",
    },
    RestRouteContract {
        method: "GET",
        specification_path: "/api/v3/notes/{note_id}/acl",
        probe_path: "/api/v3/notes/0197c9bc-0000-7000-8000-000000000001/acl",
    },
    RestRouteContract {
        method: "PUT",
        specification_path: "/api/v3/notes/{note_id}/acl",
        probe_path: "/api/v3/notes/0197c9bc-0000-7000-8000-000000000001/acl",
    },
    RestRouteContract {
        method: "GET",
        specification_path: "/api/v3/notes/{note_id}/source",
        probe_path: "/api/v3/notes/0197c9bc-0000-7000-8000-000000000001/source",
    },
    RestRouteContract {
        method: "DELETE",
        specification_path: "/api/v3/mcp-authorizations/{client_id}",
        probe_path: "/api/v3/mcp-authorizations/mcp-0197c9bc-0000-7000-8000-000000000001",
    },
];

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "NoteDraft")]
pub struct NoteDraftInput {
    #[schemars(extend("x-maxBytes" = NOTE_POLICY.max_source_bytes))]
    pub source: String,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BibliographyItemInput {
    #[schemars(extend("type" = "object"))]
    pub csl_json: Value,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "BibliographyItem")]
pub struct BibliographyItemResponse {
    #[schemars(regex(pattern = ENTITY_ID_PATTERN))]
    pub item_id: String,
    pub citation_key: String,
    #[schemars(extend("type" = "object"))]
    pub csl_json: Value,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    #[schemars(range(min = MINIMUM_REVISION))]
    pub revision: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum BibliographyImportSourceInput {
    New { display_name: String },
    Existing { source_id: String },
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BibliographyImportPreviewInput {
    pub source: BibliographyImportSourceInput,
    #[schemars(length(min = 1, max = 1000))]
    pub items: Vec<Value>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
#[schemars(rename = "BibliographyImportClassification")]
pub enum BibliographyImportClassificationResponse {
    Create,
    UpdateFromExternal,
    Unchanged,
    KeepLocal,
    Conflict,
    DuplicateCandidate,
    Rejected,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "BibliographyImportSource")]
pub struct BibliographyImportSourceResponse {
    #[schemars(regex(pattern = ENTITY_ID_PATTERN))]
    pub source_id: String,
    pub method: String,
    #[schemars(length(min = 1, max = 128))]
    pub display_name: String,
    #[schemars(range(min = MINIMUM_REVISION))]
    pub revision: i64,
    pub created_at_ms: i64,
    pub last_imported_at_ms: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "BibliographyImportCandidate")]
pub struct BibliographyImportCandidateResponse {
    #[schemars(regex(pattern = ENTITY_ID_PATTERN))]
    pub item_id: String,
    pub citation_key: String,
    #[schemars(required)]
    #[schemars(transform = nullable)]
    pub title: Option<String>,
    #[schemars(range(min = MINIMUM_REVISION))]
    pub revision: i64,
    pub matched_by: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "BibliographyImportEntry")]
pub struct BibliographyImportEntryResponse {
    pub position: usize,
    #[schemars(required)]
    #[schemars(transform = nullable)]
    pub external_item_id: Option<String>,
    #[schemars(required)]
    #[schemars(transform = nullable)]
    pub citation_key: Option<String>,
    pub classification: BibliographyImportClassificationResponse,
    #[schemars(required)]
    #[schemars(transform = nullable)]
    pub item_id: Option<String>,
    #[schemars(required)]
    #[schemars(transform = nullable)]
    #[schemars(range(min = MINIMUM_REVISION))]
    pub item_revision: Option<i64>,
    #[schemars(required)]
    #[schemars(transform = nullable)]
    #[schemars(extend("type" = "object"))]
    pub current_csl_json: Option<Value>,
    pub candidates: Vec<BibliographyImportCandidateResponse>,
    #[schemars(required)]
    #[schemars(transform = nullable)]
    pub rejection_code: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "BibliographyImportPreview")]
pub struct BibliographyImportPreviewResponse {
    #[schemars(required)]
    #[schemars(transform = nullable)]
    pub source_id: Option<String>,
    #[schemars(required)]
    #[schemars(transform = nullable)]
    #[schemars(range(min = MINIMUM_REVISION))]
    pub source_revision: Option<i64>,
    #[schemars(regex(pattern = "^[0-9a-f]{64}$"))]
    pub preview_token: String,
    pub entries: Vec<BibliographyImportEntryResponse>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
#[schemars(rename = "BibliographyImportDecisionAction")]
pub enum BibliographyImportDecisionKindInput {
    ApplySuggested,
    CreateSeparate,
    KeepLocal,
    UseExternal,
    LinkExistingKeepLocal,
    LinkExistingUseExternal,
    Exclude,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "BibliographyImportDecision")]
pub struct BibliographyImportDecisionInput {
    pub position: usize,
    pub action: BibliographyImportDecisionKindInput,
    #[schemars(required)]
    #[schemars(transform = nullable)]
    pub candidate_item_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BibliographyImportApplyInput {
    pub source: BibliographyImportSourceInput,
    #[schemars(length(min = 1, max = 1000))]
    pub items: Vec<Value>,
    #[schemars(regex(pattern = "^[0-9a-f]{64}$"))]
    pub preview_token: String,
    #[schemars(length(min = 1, max = 1000))]
    pub decisions: Vec<BibliographyImportDecisionInput>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "BibliographyImportResult")]
pub struct BibliographyImportResultResponse {
    #[schemars(regex(pattern = ENTITY_ID_PATTERN))]
    pub source_id: String,
    #[schemars(range(min = MINIMUM_REVISION))]
    pub source_revision: i64,
    pub created: usize,
    pub updated: usize,
    pub kept: usize,
    pub excluded: usize,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "MathMacro")]
pub struct MathMacroResponse {
    #[schemars(length(min = 1, max = 32), regex(pattern = "^[A-Za-z]+$"))]
    pub name: String,
    #[schemars(length(min = 1, max = 512))]
    pub replacement: String,
    #[schemars(range(max = 9))]
    pub argument_count: u8,
}

/// 数式マクロ設定。要求と応答で同じ構造を使う。
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "MathMacroSettings")]
pub struct MathMacroSettings {
    /// 全項目のコマンド名と置換内容をUTF-8 byte数で合計した上限も拡張属性で公開する。
    /// JSON配列へ符号化した後の大きさではない。
    #[schemars(
        length(max = 64),
        extend("x-marginalis-max-name-replacement-bytes" = 16384)
    )]
    pub macros: Vec<MathMacroResponse>,
    #[schemars(range(min = 0))]
    pub revision: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpScopeCeilingInput {
    pub scopes: Vec<String>,
    #[schemars(range(min = 0))]
    pub revision: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "McpScopeCeiling")]
pub struct McpScopeCeilingResponse {
    pub supported_scopes: Vec<String>,
    pub scopes: Vec<String>,
    #[schemars(range(min = 0))]
    pub revision: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "McpClientAuthorization")]
pub struct McpClientAuthorizationResponse {
    #[schemars(length(min = 1, max = 2048))]
    pub client_id: String,
    #[schemars(length(min = 1, max = 128))]
    pub display_name: String,
    pub registration_method: String,
    pub granted_scopes: Vec<String>,
    pub scope_ceiling_configured: bool,
    pub scope_ceiling: Vec<String>,
    #[schemars(range(min = 0))]
    pub scope_ceiling_revision: i64,
    pub authorized_at_ms: i64,
    #[schemars(required)]
    #[schemars(transform = nullable)]
    pub last_used_at_ms: Option<i64>,
    pub active: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "Note")]
pub struct NoteResponse {
    #[schemars(regex(pattern = ENTITY_ID_PATTERN))]
    pub note_id: String,
    pub title: String,
    pub source: String,
    pub tags: Vec<String>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    #[schemars(range(min = MINIMUM_REVISION))]
    pub revision: i64,
    pub created_via: NoteCreationSource,
    pub review_status: NoteReviewStatus,
    #[schemars(required)]
    #[schemars(transform = nullable)]
    #[schemars(range(min = MINIMUM_REVISION))]
    pub reviewed_revision: Option<i64>,
    #[schemars(required)]
    #[schemars(transform = nullable)]
    pub reviewed_at_ms: Option<i64>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "NoteSummary")]
pub struct NoteSummaryResponse {
    #[schemars(regex(pattern = ENTITY_ID_PATTERN))]
    pub note_id: String,
    pub title: String,
    pub tags: Vec<String>,
    pub updated_at_ms: i64,
    #[schemars(range(min = MINIMUM_REVISION))]
    pub revision: i64,
    pub created_via: NoteCreationSource,
    pub review_status: NoteReviewStatus,
    #[schemars(required)]
    #[schemars(transform = nullable)]
    #[schemars(range(min = MINIMUM_REVISION))]
    pub reviewed_revision: Option<i64>,
    #[schemars(required)]
    #[schemars(transform = nullable)]
    pub reviewed_at_ms: Option<i64>,
}

/// 一覧の1項目。ノート要約に実効アクセス水準を加えたもの。
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[schemars(rename = "NoteListEntry")]
pub struct NoteListEntryResponse {
    #[serde(flatten)]
    pub summary: NoteSummaryResponse,
    pub access: NoteAccess,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "NoteReview")]
pub struct NoteReviewResponse {
    #[schemars(regex(pattern = ENTITY_ID_PATTERN))]
    pub note_id: String,
    #[schemars(range(min = MINIMUM_REVISION))]
    pub current_revision: i64,
    pub status: NoteReviewStatus,
    #[schemars(required)]
    #[schemars(transform = nullable)]
    #[schemars(range(min = MINIMUM_REVISION))]
    pub reviewed_revision: Option<i64>,
    #[schemars(required)]
    #[schemars(transform = nullable)]
    pub reviewed_at_ms: Option<i64>,
    #[schemars(required)]
    #[schemars(transform = nullable)]
    pub reviewer_issuer: Option<String>,
    #[schemars(required)]
    #[schemars(transform = nullable)]
    pub reviewer_subject: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "DeletedNoteListEntry")]
pub struct DeletedNoteListEntryResponse {
    #[schemars(regex(pattern = ENTITY_ID_PATTERN))]
    pub note_id: String,
    pub title: String,
    pub deleted_at_ms: i64,
    pub purge_at_ms: i64,
    #[schemars(range(min = MINIMUM_REVISION))]
    pub revision: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "RelatedNotes")]
pub struct RelatedNotesResponse {
    pub outgoing: Vec<NoteSummaryResponse>,
    pub incoming: Vec<NoteSummaryResponse>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "NoteView")]
pub struct NoteViewResponse {
    pub note: NoteResponse,
    pub access: NoteAccess,
    pub html: String,
    pub related: RelatedNotesResponse,
    pub math_macros: Vec<MathMacroResponse>,
}

/// 関係の図に出す点と線。
///
/// 点は現在の利用者が閲覧できるノートと、そのノートが引用している文献だけを含む。線は始点と
/// 終点の両方が点として含まれる場合だけ返す。閲覧できないノートの存在も件数も現れない。
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "NoteGraph")]
pub struct NoteGraphResponse {
    pub notes: Vec<NoteGraphNoteResponse>,
    pub works: Vec<NoteGraphWorkResponse>,
    pub references: Vec<NoteGraphReferenceResponse>,
    pub citations: Vec<NoteGraphCitationResponse>,
}

/// 図に出すノート。本文は含まない。
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "NoteGraphNote")]
pub struct NoteGraphNoteResponse {
    #[schemars(regex(pattern = ENTITY_ID_PATTERN))]
    pub note_id: String,
    pub title: String,
    pub tags: Vec<String>,
    pub updated_at_ms: i64,
}

/// 図に出す文献。書誌情報そのものではなく、引用されたという事実を表す。
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "NoteGraphWork")]
pub struct NoteGraphWorkResponse {
    pub citation_key: String,
    /// 引用元のノートを書いた利用者のライブラリーで解決できた場合の題名。
    #[schemars(required)]
    #[schemars(transform = nullable)]
    pub title: Option<String>,
}

/// ノートからノートへの参照。
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "NoteGraphReference")]
pub struct NoteGraphReferenceResponse {
    #[schemars(regex(pattern = ENTITY_ID_PATTERN))]
    pub source_note_id: String,
    #[schemars(regex(pattern = ENTITY_ID_PATTERN))]
    pub target_note_id: String,
}

/// ノートから文献への引用。
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "NoteGraphCitation")]
pub struct NoteGraphCitationResponse {
    #[schemars(regex(pattern = ENTITY_ID_PATTERN))]
    pub source_note_id: String,
    pub citation_key: String,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "NoteAclEntry")]
pub struct NoteAclEntryInput {
    #[schemars(length(min = 1, max = 1024))]
    pub subject: String,
    pub permission: NotePermission,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "NoteAclUpdate")]
pub struct NoteAclUpdateInput {
    pub entries: Vec<NoteAclEntryInput>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "NoteAclGrant")]
pub struct NoteAclGrantResponse {
    pub issuer: String,
    #[schemars(length(min = 1, max = 1024))]
    pub subject: String,
    pub permission: NotePermission,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "NoteAcl")]
pub struct NoteAclResponse {
    pub entries: Vec<NoteAclGrantResponse>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "NotePreview")]
pub struct NotePreviewResponse {
    pub html: String,
    pub diagnostics: Vec<NoteDiagnosticResponse>,
    pub math_macros: Vec<MathMacroResponse>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "Session")]
pub struct SessionResponse {
    pub issuer: String,
    pub subject: String,
}

/// サーバーが初期HTMLへ埋め込み、Web UIが起動時に読む設定。
///
/// REST応答と同じく、サーバーとWeb UIの間の公開契約である。Web UI側は生成した
/// parserで検査してから使用し、解釈できない値を利用者向けエラーとして扱う。
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[schemars(rename = "ApplicationConfig")]
pub struct ApplicationConfigResponse {
    /// REST APIの外部prefix。
    pub api_base: String,
    /// 画面URLの外部prefix。サブパス配置ではその値になる。
    pub base_path: String,
    /// prefixを除いた画面内のpath。
    pub path: String,
    /// `?`を含む問い合わせ文字列。無い場合は空文字。
    pub search: String,
    /// 実行時に生成するstyleへ付けるContent Security Policyのnonce。
    pub style_nonce: String,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "Health")]
pub struct HealthResponse {
    pub status: String,
    pub api_version: String,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "Problem")]
pub struct ProblemResponse {
    pub code: ProblemCode,
    pub message: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<NoteDiagnosticResponse>,
}

impl ProblemResponse {
    /// 入力診断を伴わない失敗を組み立てる。
    pub fn new(code: ProblemCode, message: &str) -> Self {
        Self {
            code,
            message: message.to_owned(),
            diagnostics: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProblemCode {
    AuthenticationRequired,
    AuthenticationUnavailable,
    CsrfRejected,
    CsrfRequired,
    CsrfInvalid,
    SameOriginRequired,
    OriginNotAllowed,
    NotFound,
    Forbidden,
    Conflict,
    RetentionExpired,
    PreconditionRequired,
    InvalidRequest,
    ValidationFailed,
    RenderFailed,
    Unavailable,
}

impl ProblemCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AuthenticationRequired => "authentication_required",
            Self::AuthenticationUnavailable => "authentication_unavailable",
            Self::CsrfRejected => "csrf_rejected",
            Self::CsrfRequired => "csrf_required",
            Self::CsrfInvalid => "csrf_invalid",
            Self::SameOriginRequired => "same_origin_required",
            Self::OriginNotAllowed => "origin_not_allowed",
            Self::NotFound => "not_found",
            Self::Forbidden => "forbidden",
            Self::Conflict => "conflict",
            Self::RetentionExpired => "retention_expired",
            Self::PreconditionRequired => "precondition_required",
            Self::InvalidRequest => "invalid_request",
            Self::ValidationFailed => "validation_failed",
            Self::RenderFailed => "render_failed",
            Self::Unavailable => "unavailable",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "NoteDiagnostic")]
pub struct NoteDiagnosticResponse {
    pub code: String,
    pub severity: DiagnosticSeverityResponse,
    pub target: NoteValidationTarget,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub span: Option<Utf8ByteSpanResponse>,
    pub message: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
#[schemars(rename = "DiagnosticSeverity")]
pub enum DiagnosticSeverityResponse {
    Error,
    Warning,
    Information,
    Hint,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "Utf8ByteSpan")]
pub struct Utf8ByteSpanResponse {
    pub start: u32,
    pub end: u32,
    pub unit: Utf8ByteUnit,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Utf8ByteUnit {
    Utf8Byte,
}

/// 公開契約schemaの正本であるRust型から``components.schemas``を生成する。
///
/// ここへ型を足すと、OpenAPIとTypeScript契約の両方へ同じschemaが出力される。
pub(crate) fn component_schemas() -> Value {
    let mut settings = SchemaSettings::draft2020_12();
    settings.definitions_path = "#/components/schemas/".into();
    let mut generator = SchemaGenerator::new(settings);
    generator.subschema_for::<HealthResponse>();
    generator.subschema_for::<SessionResponse>();
    generator.subschema_for::<ApplicationConfigResponse>();
    generator.subschema_for::<MathMacroSettings>();
    generator.subschema_for::<McpScopeCeilingInput>();
    generator.subschema_for::<McpScopeCeilingResponse>();
    generator.subschema_for::<McpClientAuthorizationResponse>();
    generator.subschema_for::<NoteDraftInput>();
    generator.subschema_for::<BibliographyItemInput>();
    generator.subschema_for::<BibliographyItemResponse>();
    generator.subschema_for::<BibliographyImportSourceInput>();
    generator.subschema_for::<BibliographyImportPreviewInput>();
    generator.subschema_for::<BibliographyImportSourceResponse>();
    generator.subschema_for::<BibliographyImportPreviewResponse>();
    generator.subschema_for::<BibliographyImportApplyInput>();
    generator.subschema_for::<BibliographyImportResultResponse>();
    generator.subschema_for::<NoteResponse>();
    generator.subschema_for::<NoteSummaryResponse>();
    generator.subschema_for::<NoteListEntryResponse>();
    generator.subschema_for::<NoteReviewResponse>();
    generator.subschema_for::<DeletedNoteListEntryResponse>();
    generator.subschema_for::<NoteViewResponse>();
    generator.subschema_for::<NoteGraphResponse>();
    generator.subschema_for::<NotePreviewResponse>();
    generator.subschema_for::<NoteAclUpdateInput>();
    generator.subschema_for::<NoteAclResponse>();
    generator.subschema_for::<ProblemResponse>();
    Value::Object(generator.take_definitions(true))
}

pub fn openapi_document() -> Value {
    json!({
        "openapi": "3.1.0",
        "info": {
            "title": "Marginalis REST API",
            "version": API_VERSION,
            "x-adocweave-package-version": "0.40.0",
            "x-note-profile-version": 15
        },
        "paths": rest_paths(),
        "components": {
            "parameters": {
                "NoteId": {"name": "note_id", "in": "path", "required": true, "schema": note_id_schema()},
                "BibliographyQuery": {"name": "query", "in": "query", "required": false,
                    "schema": {"type": "string", "maxLength": 256},
                    "description": "citation key、題名、著者、DOIのいずれかにこの語を含む文献だけへ絞る"},
                "GraphQuery": {"name": "query", "in": "query", "required": false, "schema": {"type": "string"},
                    "description": "題名、本文、タグのいずれかにこの語を含むノートだけへ絞る"},
                "GraphOrigin": {"name": "origin", "in": "query", "required": false, "schema": note_id_schema(),
                    "description": "起点のノート。指定するとそこからdepth階層以内だけを返す"},
                "GraphDepth": {"name": "depth", "in": "query", "required": false,
                    "schema": {"type": "integer", "minimum": 1, "maximum": MAX_GRAPH_DEPTH},
                    "description": "起点から辿る線の本数。originを指定した場合だけ使う。既定は1"},
                "CreationSource": {"name": "created_via", "in": "query", "required": false,
                    "schema": {"$ref": "#/components/schemas/NoteCreationSource"}},
                "ReviewStatus": {"name": "review_status", "in": "query", "required": false,
                    "schema": {"$ref": "#/components/schemas/NoteReviewStatus"}},
                "McpScopeCeilingRevision": {"name": "revision", "in": "query", "required": true,
                    "schema": {"type": "integer", "minimum": 1},
                    "description": "解除する上限のrevision。現在の値と一致しない場合は409を返す"},
                "CsrfToken": {"name": "X-CSRF-Token", "in": "header", "required": true, "schema": {"type": "string", "minLength": 1}},
                "IfMatch": {"name": "If-Match", "in": "header", "required": true, "schema": {"type": "string", "pattern": "^\\\"rev-[1-9][0-9]*\\\"$"}}
            },
            "schemas": component_schemas(),
            "responses": {
                "NotFound": problem_response("note or authorization is not visible"),
                "Conflict": problem_response("the If-Match revision is stale"),
                "RetentionExpired": problem_response("the note restoration period has expired"),
                "PreconditionRequired": problem_response("If-Match is required"),
                "BadRequest": problem_response("the request syntax or If-Match value is invalid"),
                "AuthenticationRequired": problem_response("OIDC session is required"),
                "CsrfRejected": problem_response("same-origin or CSRF validation failed"),
                "Unavailable": problem_response("the service is temporarily unavailable"),
                "ValidationFailed": problem_response("note input is invalid"),
                "UnprocessableNote": problem_response("the note input is invalid or cannot be rendered safely")
            }
        }
    })
}

fn rest_paths() -> Value {
    json!({
        "/api/v3/health": {
            "get": operation("Liveness check", &[], None, responses(&[("200", schema_response("service is running", "Health"))]))
        },
        "/api/v3/session": {
            "get": operation("Read the current identity", &[], None, responses(&[
                ("200", schema_response("authenticated session", "Session")),
                ("401", response_ref("AuthenticationRequired"))
            ]))
        },
        "/api/v3/math-macros": {
            "get": operation("Read the current user's MathJax macros", &[], None, responses(&[
                ("200", schema_response("MathJax macro settings", "MathMacroSettings")),
                ("401", response_ref("AuthenticationRequired")),
                ("503", response_ref("Unavailable"))
            ])),
            "put": operation("Replace the current user's MathJax macros", &["CsrfToken"], Some("MathMacroSettings"), responses(&[
                ("200", schema_response("updated MathJax macro settings", "MathMacroSettings")),
                ("401", response_ref("AuthenticationRequired")),
                ("403", response_ref("CsrfRejected")),
                ("409", response_ref("Conflict")),
                ("422", response_ref("ValidationFailed")),
                ("503", response_ref("Unavailable"))
            ]))
        },
        "/api/v3/mcp-scope-ceilings": {
            "get": operation("Read the current user's MCP scope ceiling", &[], None, responses(&[
                ("200", schema_response("MCP scope ceiling", "McpScopeCeiling")),
                ("401", response_ref("AuthenticationRequired")),
                ("503", response_ref("Unavailable"))
            ])),
            "put": operation("Replace the current user's MCP scope ceiling", &["CsrfToken"], Some("McpScopeCeilingInput"), responses(&[
                ("200", schema_response("updated MCP scope ceiling", "McpScopeCeiling")),
                ("401", response_ref("AuthenticationRequired")),
                ("403", response_ref("CsrfRejected")),
                ("409", response_ref("Conflict")),
                ("422", response_ref("ValidationFailed")),
                ("503", response_ref("Unavailable"))
            ]))
        },
        "/api/v3/mcp-authorizations": {
            "get": operation("List the current user's MCP client authorizations", &[], None, responses(&[
                ("200", array_response("MCP client authorizations", "McpClientAuthorization")),
                ("401", response_ref("AuthenticationRequired")),
                ("503", response_ref("Unavailable"))
            ]))
        },
        "/api/v3/mcp-authorizations/{client_id}/scope-ceiling": {
            "parameters": [{
                "name": "client_id", "in": "path", "required": true,
                "schema": {"type": "string", "minLength": 1, "maxLength": 2048}
            }],
            "put": operation("Restrict one MCP client's scope ceiling", &["CsrfToken"], Some("McpScopeCeilingInput"), responses(&[
                ("200", schema_response("updated MCP client authorization", "McpClientAuthorization")),
                ("400", response_ref("BadRequest")),
                ("401", response_ref("AuthenticationRequired")),
                ("403", response_ref("CsrfRejected")),
                ("404", response_ref("NotFound")),
                ("409", response_ref("Conflict")),
                ("503", response_ref("Unavailable"))
            ])),
            "delete": operation("Remove one MCP client's scope ceiling", &["McpScopeCeilingRevision", "CsrfToken"], None, responses(&[
                ("200", schema_response("MCP client authorization without a configured ceiling", "McpClientAuthorization")),
                ("400", response_ref("BadRequest")),
                ("401", response_ref("AuthenticationRequired")),
                ("403", response_ref("CsrfRejected")),
                ("404", response_ref("NotFound")),
                ("409", response_ref("Conflict")),
                ("422", response_ref("ValidationFailed")),
                ("503", response_ref("Unavailable"))
            ]))
        },
        "/api/v3/notes": {
            "get": operation("List visible note summaries", &["CreationSource", "ReviewStatus"], None, responses(&[
                ("200", array_response("visible note summaries", "NoteListEntry")),
                ("401", response_ref("AuthenticationRequired"))
            ])),
            "post": operation("Create a note", &["CsrfToken"], Some("NoteDraft"), responses(&[
                ("201", schema_response_with_etag("created note", "Note")),
                ("401", response_ref("AuthenticationRequired")),
                ("403", response_ref("CsrfRejected")),
                ("422", response_ref("UnprocessableNote"))
            ]))
        },
        "/api/v3/web/notes": {
            "post": operation("Create a note from the Web UI", &["CsrfToken"], Some("NoteDraft"), responses(&[
                ("201", schema_response_with_etag("created note", "Note")),
                ("401", response_ref("AuthenticationRequired")),
                ("403", response_ref("CsrfRejected")),
                ("422", response_ref("UnprocessableNote"))
            ]))
        },
        "/api/v3/notes/deleted": {
            "get": operation("List deleted notes owned by the current user", &[], None, responses(&[
                ("200", array_response("owned deleted note summaries", "DeletedNoteListEntry")),
                ("401", response_ref("AuthenticationRequired")),
                ("503", response_ref("Unavailable"))
            ]))
        },
        "/api/v3/bibliography": {
            "get": operation("Search the current user's bibliography", &["BibliographyQuery"], None, responses(&[
                ("200", array_response("bibliography items", "BibliographyItem")),
                ("400", response_ref("BadRequest")),
                ("401", response_ref("AuthenticationRequired"))
            ])),
            "post": operation("Add one CSL-JSON bibliography item", &["CsrfToken"], Some("BibliographyItemInput"), responses(&[
                ("201", schema_response_with_etag("created bibliography item", "BibliographyItem")),
                ("401", response_ref("AuthenticationRequired")),
                ("403", response_ref("CsrfRejected")),
                ("409", response_ref("Conflict")),
                ("422", response_ref("ValidationFailed"))
            ]))
        },
        "/api/v3/bibliography/{item_id}": {
            "parameters": [{
                "name": "item_id", "in": "path", "required": true,
                "schema": note_id_schema()
            }],
            "put": operation("Update an owned CSL-JSON bibliography item", &["CsrfToken", "IfMatch"], Some("BibliographyItemInput"), responses(&[
                ("200", schema_response_with_etag("updated bibliography item", "BibliographyItem")),
                ("404", response_ref("NotFound")),
                ("409", response_ref("Conflict")),
                ("422", response_ref("ValidationFailed")),
                ("428", response_ref("PreconditionRequired"))
            ])),
            "delete": operation("Delete an owned bibliography item", &["CsrfToken", "IfMatch"], None, responses(&[
                ("204", json!({"description": "bibliography item deleted"})),
                ("404", response_ref("NotFound")),
                ("409", response_ref("Conflict")),
                ("428", response_ref("PreconditionRequired"))
            ]))
        },
        "/api/v3/bibliography/import-sources": {
            "get": operation("List bibliography import sources owned by the current user", &[], None, responses(&[
                ("200", array_response("bibliography import sources", "BibliographyImportSource")),
                ("401", response_ref("AuthenticationRequired")),
                ("503", response_ref("Unavailable"))
            ]))
        },
        "/api/v3/bibliography/import-previews": {
            "post": operation("Preview a CSL-JSON bibliography import without changing stored data", &[], Some("BibliographyImportPreviewInput"), responses(&[
                ("200", schema_response("bibliography import preview", "BibliographyImportPreview")),
                ("400", response_ref("BadRequest")),
                ("401", response_ref("AuthenticationRequired")),
                ("404", response_ref("NotFound")),
                ("422", response_ref("ValidationFailed")),
                ("503", response_ref("Unavailable"))
            ]))
        },
        "/api/v3/bibliography/imports": {
            "post": operation("Apply a previewed CSL-JSON bibliography import atomically", &["CsrfToken"], Some("BibliographyImportApplyInput"), responses(&[
                ("200", schema_response("bibliography import result", "BibliographyImportResult")),
                ("400", response_ref("BadRequest")),
                ("401", response_ref("AuthenticationRequired")),
                ("403", response_ref("CsrfRejected")),
                ("404", response_ref("NotFound")),
                ("409", response_ref("Conflict")),
                ("422", response_ref("ValidationFailed")),
                ("503", response_ref("Unavailable"))
            ]))
        },
        "/api/v3/mcp-authorizations/{client_id}": {
            "parameters": [{
                "name": "client_id", "in": "path", "required": true,
                "schema": {"type": "string", "minLength": 1, "maxLength": 2048}
            }],
            "delete": operation("Revoke every MCP token issued to one client", &["CsrfToken"], None, responses(&[
                ("204", json!({"description": "MCP authorization revoked"})),
                ("400", response_ref("BadRequest")),
                ("401", response_ref("AuthenticationRequired")),
                ("403", response_ref("CsrfRejected")),
                ("404", response_ref("NotFound")),
                ("503", response_ref("Unavailable"))
            ]))
        },
        "/api/v3/notes/preview": {
            "post": operation("Validate and render a new unsaved note", &["CsrfToken"], Some("NoteDraft"), responses(&[
                ("200", schema_response("safe HTML preview", "NotePreview")),
                ("401", response_ref("AuthenticationRequired")),
                ("403", response_ref("CsrfRejected")),
                ("422", response_ref("UnprocessableNote"))
            ]))
        },
        "/api/v3/notes/{note_id}/preview": {
            "parameters": [parameter_ref("NoteId")],
            "post": operation("Validate and render an unsaved update", &["CsrfToken"], Some("NoteDraft"), responses(&[
                ("200", schema_response("safe HTML preview", "NotePreview")),
                ("401", response_ref("AuthenticationRequired")),
                ("403", response_ref("CsrfRejected")),
                ("404", response_ref("NotFound")),
                ("422", response_ref("UnprocessableNote"))
            ]))
        },
        "/api/v3/notes/{note_id}": {
            "parameters": [parameter_ref("NoteId")],
            "get": operation("Read one visible note", &[], None, responses(&[
                ("200", schema_response_with_etag("note", "Note")),
                ("404", response_ref("NotFound"))
            ])),
            "put": operation("Update a note", &["CsrfToken", "IfMatch"], Some("NoteDraft"), mutation_responses("updated note")),
            "delete": operation("Soft-delete a note", &["CsrfToken", "IfMatch"], None, mutation_responses("soft-deleted note"))
        },
        "/api/v3/notes/{note_id}/restore": {
            "parameters": [parameter_ref("NoteId")],
            "post": operation("Restore a note", &["CsrfToken", "IfMatch"], None, responses(&[
                ("200", schema_response_with_etag("restored note", "Note")),
                ("404", response_ref("NotFound")),
                ("409", response_ref("Conflict")),
                ("410", response_ref("RetentionExpired")),
                ("428", response_ref("PreconditionRequired")),
                ("400", response_ref("BadRequest")),
                ("503", response_ref("Unavailable"))
            ]))
        },
        "/api/v3/notes/graph": {
            "get": operation(
                "Read the graph of visible notes and the works they cite",
                &["GraphQuery", "GraphOrigin", "GraphDepth"],
                None,
                responses(&[
                    ("200", schema_response("note graph", "NoteGraph")),
                    ("400", response_ref("BadRequest")),
                ]),
            )
        },
        "/api/v3/notes/{note_id}/view": {
            "parameters": [parameter_ref("NoteId")],
            "get": operation("Read one coherent note view", &[], None, responses(&[
                ("200", schema_response_with_etag("rendered note view", "NoteView")),
                ("404", response_ref("NotFound")),
                ("422", response_ref("ValidationFailed"))
            ]))
        },
        "/api/v3/notes/{note_id}/acl": {
            "parameters": [parameter_ref("NoteId")],
            "get": operation("Read note ACL", &[], None, responses(&[
                ("200", schema_response_with_etag("ACL entries", "NoteAcl")),
                ("404", response_ref("NotFound"))
            ])),
            "put": operation("Replace note ACL", &["CsrfToken", "IfMatch"], Some("NoteAclUpdate"), mutation_responses("note with updated ACL"))
        },
        "/api/v3/notes/{note_id}/review": {
            "parameters": [parameter_ref("NoteId")],
            "get": operation("Read the owned note review record", &[], None, responses(&[
                ("200", schema_response_with_etag("note review", "NoteReview")),
                ("404", response_ref("NotFound")),
                ("503", response_ref("Unavailable"))
            ])),
            "post": operation("Mark the current note revision as reviewed", &["CsrfToken", "IfMatch"], None, responses(&[
                ("200", schema_response_with_etag("updated note review", "NoteReview")),
                ("404", response_ref("NotFound")),
                ("409", response_ref("Conflict")),
                ("428", response_ref("PreconditionRequired")),
                ("503", response_ref("Unavailable"))
            ]))
        },
        "/api/v3/notes/{note_id}/source": {
            "parameters": [parameter_ref("NoteId")],
            "get": {
                "summary": "Export canonical AsciiDoc",
                "responses": {"200": {"description": "AsciiDoc source", "content": {"text/asciidoc": {"schema": {"type": "string"}}}}}
            }
        }
    })
}

fn operation(summary: &str, parameters: &[&str], body: Option<&str>, responses: Value) -> Value {
    let mut value = json!({"summary": summary, "responses": responses});
    if !parameters.is_empty() {
        value["parameters"] =
            Value::Array(parameters.iter().map(|name| parameter_ref(name)).collect());
    }
    if let Some(schema) = body {
        value["requestBody"] = json!({
            "required": true,
            "content": {"application/json": {"schema": {"$ref": format!("#/components/schemas/{schema}")}}}
        });
    }
    value
}

fn mutation_responses(description: &str) -> Value {
    responses(&[
        ("200", schema_response_with_etag(description, "Note")),
        ("404", response_ref("NotFound")),
        ("409", response_ref("Conflict")),
        ("428", response_ref("PreconditionRequired")),
        ("400", response_ref("BadRequest")),
        ("422", response_ref("UnprocessableNote")),
    ])
}

fn responses(entries: &[(&str, Value)]) -> Value {
    Value::Object(
        entries
            .iter()
            .map(|(status, response)| ((*status).to_owned(), response.clone()))
            .collect(),
    )
}

fn schema_response(description: &str, schema: &str) -> Value {
    json!({"description": description, "content": {"application/json": {"schema": {"$ref": format!("#/components/schemas/{schema}")}}}})
}

fn schema_response_with_etag(description: &str, schema: &str) -> Value {
    let mut response = schema_response(description, schema);
    response["headers"] = json!({"ETag": {"schema": {"type": "string"}}});
    response
}

fn array_response(description: &str, schema: &str) -> Value {
    json!({"description": description, "content": {"application/json": {"schema": {"type": "array", "items": {"$ref": format!("#/components/schemas/{schema}")}}}}})
}

fn response_ref(name: &str) -> Value {
    json!({"$ref": format!("#/components/responses/{name}")})
}

fn parameter_ref(name: &str) -> Value {
    json!({"$ref": format!("#/components/parameters/{name}")})
}

fn problem_response(description: &str) -> Value {
    json!({"description": description, "content": {"application/json": {"schema": {"$ref": "#/components/schemas/Problem"}}}})
}

fn note_id_schema() -> Value {
    json!({"type": "string", "format": "uuid", "pattern": ENTITY_ID_PATTERN})
}

#[cfg(test)]
mod tests {
    use super::*;

    /// domainが持つ語彙の公開JSON表現を固定する。
    ///
    /// domain側で識別子や`serde`属性を変更すると、この試験が公開表現の変化として検出する。
    /// 公開表現を意図して変える場合だけ、この期待値と生成物を同じ変更で更新する。
    #[test]
    fn domain_vocabulary_keeps_its_public_json_representation() {
        let permissions = [
            (NotePermission::Read, "read"),
            (NotePermission::Edit, "edit"),
        ];
        for (value, expected) in permissions {
            assert_eq!(serde_json::to_value(value).expect("permission"), expected);
        }

        let accesses = [
            (NoteAccess::Read, "read"),
            (NoteAccess::Edit, "edit"),
            (NoteAccess::Manage, "manage"),
        ];
        for (value, expected) in accesses {
            assert_eq!(serde_json::to_value(value).expect("access"), expected);
        }

        let targets = [
            (NoteValidationTarget::Source, json!({"field": "source"})),
            (NoteValidationTarget::Title, json!({"field": "title"})),
            (NoteValidationTarget::Body, json!({"field": "body"})),
            (NoteValidationTarget::Tags, json!({"field": "tags"})),
            (
                NoteValidationTarget::Tag { index: 2 },
                json!({"field": "tag", "index": 2}),
            ),
            (
                NoteValidationTarget::AclEntry { index: 3 },
                json!({"field": "acl_entry", "index": 3}),
            ),
        ];
        for (value, expected) in targets {
            assert_eq!(serde_json::to_value(&value).expect("target"), expected);
            assert_eq!(
                serde_json::from_value::<NoteValidationTarget>(expected).expect("target"),
                value
            );
        }
    }

    /// ノートIDとrevisionのJSON Schemaが、domainの規則を出典としていることを確認する。
    #[test]
    fn generated_schemas_reference_domain_identifier_rules() {
        assert_eq!(note_id_schema()["pattern"], ENTITY_ID_PATTERN);
        let schemas = component_schemas();
        assert_eq!(
            schemas["Note"]["properties"]["note_id"]["pattern"],
            ENTITY_ID_PATTERN
        );
        assert_eq!(
            schemas["Note"]["properties"]["revision"]["minimum"],
            Revision::MINIMUM_VALUE
        );
    }

    #[test]
    fn generated_contracts_use_one_api_version_and_conditional_updates() {
        let document = openapi_document();
        assert_eq!(document["info"]["version"], API_VERSION);
        assert_eq!(
            document["components"]["parameters"]["BibliographyQuery"]["schema"]["maxLength"],
            256
        );
        assert!(
            document["paths"]["/api/v3/bibliography"]["get"]["responses"]
                .get("400")
                .is_some()
        );
        assert!(
            document["paths"]["/api/v3/notes/{note_id}"]["put"]["parameters"]
                .as_array()
                .expect("parameters")
                .iter()
                .any(|parameter| parameter["$ref"] == "#/components/parameters/IfMatch")
        );
        for route in REST_ROUTE_CONTRACTS {
            assert!(
                document["paths"][route.specification_path]
                    .get(route.method.to_ascii_lowercase())
                    .is_some(),
                "{} {} is missing from OpenAPI",
                route.method,
                route.specification_path
            );
        }
    }
}
