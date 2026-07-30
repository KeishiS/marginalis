//! REST APIとTypeScriptクライアントで共有する公開契約。

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub const API_VERSION: &str = "v3";
pub const API_PREFIX: &str = "/api/v3";

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
        specification_path: "/api/v3/notes",
        probe_path: "/api/v3/notes",
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
        method: "POST",
        specification_path: "/api/v3/notes",
        probe_path: "/api/v3/notes",
    },
    RestRouteContract {
        method: "POST",
        specification_path: "/api/v3/notes/preview",
        probe_path: "/api/v3/notes/preview",
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
];

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NoteDraftInput {
    pub source: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BibliographyItemInput {
    pub csl_json: Value,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BibliographyItemResponse {
    pub item_id: String,
    pub citation_key: String,
    pub csl_json: Value,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub revision: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NoteResponse {
    pub note_id: String,
    pub title: String,
    pub source: String,
    pub tags: Vec<String>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub revision: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NoteSummaryResponse {
    pub note_id: String,
    pub title: String,
    pub tags: Vec<String>,
    pub updated_at_ms: i64,
    pub revision: i64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NoteAccessValue {
    Read,
    Edit,
    Manage,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NoteListEntryResponse {
    pub note_id: String,
    pub title: String,
    pub tags: Vec<String>,
    pub updated_at_ms: i64,
    pub revision: i64,
    pub access: NoteAccessValue,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RelatedNotesResponse {
    pub outgoing: Vec<NoteSummaryResponse>,
    pub incoming: Vec<NoteSummaryResponse>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NoteViewResponse {
    pub note: NoteResponse,
    pub access: NoteAccessValue,
    pub html: String,
    pub related: RelatedNotesResponse,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NotePermissionValue {
    Read,
    Edit,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NoteAclEntryInput {
    pub subject: String,
    pub permission: NotePermissionValue,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NoteAclUpdateInput {
    pub entries: Vec<NoteAclEntryInput>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NoteAclGrantResponse {
    pub issuer: String,
    pub subject: String,
    pub permission: NotePermissionValue,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NoteAclResponse {
    pub entries: Vec<NoteAclGrantResponse>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NotePreviewResponse {
    pub html: String,
    pub diagnostics: Vec<NoteDiagnosticResponse>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SessionResponse {
    pub issuer: String,
    pub subject: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HealthResponse {
    pub status: String,
    pub api_version: String,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProblemResponse {
    pub code: ProblemCode,
    pub message: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<NoteDiagnosticResponse>,
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
    PreconditionRequired,
    InvalidRequest,
    ValidationFailed,
    RenderFailed,
    Unavailable,
}

impl ProblemCode {
    const ALL: [Self; 15] = [
        Self::AuthenticationRequired,
        Self::AuthenticationUnavailable,
        Self::CsrfRejected,
        Self::CsrfRequired,
        Self::CsrfInvalid,
        Self::SameOriginRequired,
        Self::OriginNotAllowed,
        Self::NotFound,
        Self::Forbidden,
        Self::Conflict,
        Self::PreconditionRequired,
        Self::InvalidRequest,
        Self::ValidationFailed,
        Self::RenderFailed,
        Self::Unavailable,
    ];

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
pub struct NoteDiagnosticResponse {
    pub code: String,
    pub severity: DiagnosticSeverityResponse,
    pub target: ValidationTargetResponse,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub span: Option<Utf8ByteSpanResponse>,
    pub message: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticSeverityResponse {
    Error,
    Warning,
    Information,
    Hint,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "field", rename_all = "snake_case", deny_unknown_fields)]
pub enum ValidationTargetResponse {
    Source,
    Title,
    Body,
    Tag { index: usize },
    Tags,
    AclEntry { index: usize },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
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

pub fn openapi_document() -> Value {
    let note = json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["note_id", "title", "source", "tags", "created_at_ms", "updated_at_ms", "revision"],
        "properties": {
            "note_id": note_id_schema(),
            "title": {"type": "string"},
            "source": {"type": "string"},
            "tags": {"type": "array", "items": {"type": "string"}},
            "created_at_ms": {"type": "integer"},
            "updated_at_ms": {"type": "integer"},
            "revision": revision_schema()
        }
    });
    let note_summary = json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["note_id", "title", "tags", "updated_at_ms", "revision"],
        "properties": {
            "note_id": note_id_schema(),
            "title": {"type": "string"},
            "tags": {"type": "array", "items": {"type": "string"}},
            "updated_at_ms": {"type": "integer"},
            "revision": revision_schema()
        }
    });
    let note_list_entry = json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["note_id", "title", "tags", "updated_at_ms", "revision", "access"],
        "properties": {
            "note_id": note_id_schema(),
            "title": {"type": "string"},
            "tags": {"type": "array", "items": {"type": "string"}},
            "updated_at_ms": {"type": "integer"},
            "revision": revision_schema(),
            "access": {"enum": ["read", "edit", "manage"]}
        }
    });
    json!({
        "openapi": "3.1.0",
        "info": {
            "title": "Marginalis REST API",
            "version": API_VERSION,
            "x-adocweave-package-version": "0.20.0",
            "x-note-profile-version": 6
        },
        "paths": rest_paths(),
        "components": {
            "parameters": {
                "NoteId": {"name": "note_id", "in": "path", "required": true, "schema": note_id_schema()},
                "CsrfToken": {"name": "X-CSRF-Token", "in": "header", "required": true, "schema": {"type": "string", "minLength": 1}},
                "IfMatch": {"name": "If-Match", "in": "header", "required": true, "schema": {"type": "string", "pattern": "^\\\"rev-[1-9][0-9]*\\\"$"}}
            },
            "schemas": {
                "Health": {
                    "type": "object", "additionalProperties": false,
                    "required": ["status", "api_version"],
                    "properties": {"status": {"const": "ok"}, "api_version": {"const": API_VERSION}}
                },
                "Session": {
                    "type": "object", "additionalProperties": false,
                    "required": ["issuer", "subject"],
                    "properties": {"issuer": {"type": "string", "format": "uri"}, "subject": {"type": "string"}}
                },
                "NoteDraft": note_draft_schema(),
                "BibliographyItemInput": {
                    "type": "object", "additionalProperties": false,
                    "required": ["csl_json"],
                    "properties": {"csl_json": {"type": "object"}}
                },
                "BibliographyItem": {
                    "type": "object", "additionalProperties": false,
                    "required": ["item_id", "citation_key", "csl_json", "created_at_ms", "updated_at_ms", "revision"],
                    "properties": {
                        "item_id": note_id_schema(),
                        "citation_key": {"type": "string"},
                        "csl_json": {"type": "object"},
                        "created_at_ms": {"type": "integer"},
                        "updated_at_ms": {"type": "integer"},
                        "revision": revision_schema()
                    }
                },
                "Note": note,
                "NoteSummary": note_summary,
                "NoteListEntry": note_list_entry,
                "NoteView": {
                    "type": "object", "additionalProperties": false,
                    "required": ["note", "access", "html", "related"],
                    "properties": {
                        "note": {"$ref": "#/components/schemas/Note"},
                        "access": {"enum": ["read", "edit", "manage"]},
                        "html": {"type": "string"},
                        "related": {
                            "type": "object", "additionalProperties": false,
                            "required": ["outgoing", "incoming"],
                            "properties": {
                                "outgoing": {"type": "array", "items": {"$ref": "#/components/schemas/NoteSummary"}},
                                "incoming": {"type": "array", "items": {"$ref": "#/components/schemas/NoteSummary"}}
                            }
                        }
                    }
                },
                "NotePreview": {
                    "type": "object", "additionalProperties": false,
                    "required": ["html", "diagnostics"],
                    "properties": {
                        "html": {"type": "string"},
                        "diagnostics": {
                            "type": "array",
                            "items": {"$ref": "#/components/schemas/NoteDiagnostic"}
                        }
                    }
                },
                "NoteDiagnostic": note_diagnostic_schema(),
                "NoteAclEntry": {
                    "type": "object", "additionalProperties": false, "required": ["subject", "permission"],
                    "properties": {
                        "subject": {"type": "string", "minLength": 1, "maxLength": 1024},
                        "permission": {"enum": ["read", "edit"]}
                    }
                },
                "NoteAclGrant": {
                    "type": "object", "additionalProperties": false, "required": ["issuer", "subject", "permission"],
                    "properties": {
                        "issuer": {"type": "string", "format": "uri", "maxLength": 2048},
                        "subject": {"type": "string", "minLength": 1, "maxLength": 1024},
                        "permission": {"enum": ["read", "edit"]}
                    }
                },
                "NoteAcl": {
                    "type": "object", "additionalProperties": false, "required": ["entries"],
                    "properties": {"entries": {"type": "array", "items": {"$ref": "#/components/schemas/NoteAclGrant"}}}
                },
                "NoteAclUpdate": {
                    "type": "object", "additionalProperties": false, "required": ["entries"],
                    "properties": {"entries": {"type": "array", "items": {"$ref": "#/components/schemas/NoteAclEntry"}}}
                },
                "Problem": problem_schema()
            },
            "responses": {
                "NotFound": problem_response("note or authorization is not visible"),
                "Conflict": problem_response("the If-Match revision is stale"),
                "PreconditionRequired": problem_response("If-Match is required"),
                "BadRequest": problem_response("the request syntax or If-Match value is invalid"),
                "AuthenticationRequired": problem_response("OIDC session is required"),
                "CsrfRejected": problem_response("same-origin or CSRF validation failed"),
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
        "/api/v3/notes": {
            "get": operation("List visible note summaries", &[], None, responses(&[
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
        "/api/v3/bibliography": {
            "get": operation("Search the current user's bibliography", &[], None, responses(&[
                ("200", array_response("bibliography items", "BibliographyItem")),
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
        "/api/v3/notes/preview": {
            "post": operation("Validate and render an unsaved note", &["CsrfToken"], Some("NoteDraft"), responses(&[
                ("200", schema_response("safe HTML preview", "NotePreview")),
                ("401", response_ref("AuthenticationRequired")),
                ("403", response_ref("CsrfRejected")),
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
            "post": operation("Restore a note", &["CsrfToken", "IfMatch"], None, mutation_responses("restored note"))
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

fn problem_schema() -> Value {
    let codes = ProblemCode::ALL
        .into_iter()
        .map(|code| serde_json::to_value(code).expect("problem code is serializable"))
        .collect::<Vec<_>>();
    json!({
        "type": "object", "additionalProperties": false, "required": ["code", "message"],
        "properties": {
            "code": {"enum": codes},
            "message": {"type": "string"},
            "diagnostics": {
                "type": "array",
                "items": {"$ref": "#/components/schemas/NoteDiagnostic"}
            }
        }
    })
}

fn note_diagnostic_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["code", "severity", "target", "message"],
        "properties": {
            "code": {"type": "string"},
            "severity": {"enum": ["error", "warning", "information", "hint"]},
            "target": {
                "oneOf": [
                    {
                        "type": "object",
                        "additionalProperties": false,
                        "required": ["field"],
                        "properties": {
                            "field": {"enum": ["source", "title", "body", "tags"]}
                        }
                    },
                    {
                        "type": "object",
                        "additionalProperties": false,
                        "required": ["field", "index"],
                        "properties": {
                            "field": {"enum": ["tag", "acl_entry"]},
                            "index": {"type": "integer", "minimum": 0}
                        }
                    }
                ]
            },
            "span": {
                "type": "object",
                "additionalProperties": false,
                "required": ["start", "end", "unit"],
                "properties": {
                    "start": {"type": "integer", "minimum": 0},
                    "end": {"type": "integer", "minimum": 0},
                    "unit": {"const": "utf8_byte"}
                }
            },
            "message": {"type": "string"}
        }
    })
}

fn note_draft_schema() -> Value {
    object_schema(
        json!({
            "source": {"type": "string", "x-maxBytes": 524288}
        }),
        &["source"],
    )
}

fn note_id_schema() -> Value {
    json!({"type": "string", "format": "uuid", "pattern": "^[0-9a-fA-F-]{36}$"})
}

fn revision_schema() -> Value {
    json!({"type": "integer", "minimum": 1})
}

fn object_schema(properties: Value, required: &[&str]) -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": required,
        "properties": properties
    })
}

pub fn typescript_contracts() -> &'static str {
    include_str!("typescript-contracts.ts")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_contracts_use_one_api_version_and_conditional_updates() {
        let document = openapi_document();
        assert_eq!(document["info"]["version"], API_VERSION);
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
