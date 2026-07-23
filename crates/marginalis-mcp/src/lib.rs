//! MCP JSON-RPCのtool adapter。HTTP、OAuthおよびSQLiteには依存しない。

use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use marginalis_application::{NoteDraft, NoteUseCaseError, NoteUseCases};
use marginalis_domain::{Actor, EntityId, NoteId, SourceRevision};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{str::FromStr, sync::Arc};

pub const MCP_PROTOCOL_VERSION: &str = "2025-11-25";

/// bearer tokenを検証済みの一般ユーザーへ変換するMCP専用の境界。
#[async_trait]
pub trait McpAuthenticator: Send + Sync {
    async fn authenticate(
        &self,
        bearer_token: &str,
        required_scope: &str,
    ) -> Result<Actor, McpAuthenticationError>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum McpAuthenticationError {
    MissingOrInvalid,
    InsufficientScope,
    Unavailable,
}

/// read-only MCP toolの実装。書込みtoolはOAuth scopeと変更commandを確定後に追加する。
#[derive(Clone)]
pub struct McpTools {
    notes: Arc<dyn NoteUseCases>,
}

impl McpTools {
    pub const fn new(notes: Arc<dyn NoteUseCases>) -> Self {
        Self { notes }
    }

    pub async fn handle(&self, actor: Actor, request: JsonRpcRequest) -> Option<JsonRpcResponse> {
        let id = request.id.clone().unwrap_or(Value::Null);
        if request.jsonrpc != "2.0" {
            return request
                .id
                .map(|id| JsonRpcResponse::error(id, -32600, "JSON-RPC version is invalid"));
        }
        let response = match request.method.as_str() {
            "initialize" => JsonRpcResponse::success(
                id,
                json!({
                    "protocolVersion": MCP_PROTOCOL_VERSION,
                    "capabilities": { "tools": {} },
                    "serverInfo": { "name": "marginalis", "version": env!("CARGO_PKG_VERSION") }
                }),
            ),
            "tools/list" => JsonRpcResponse::success(id, tool_list()),
            "tools/call" => self.call_tool(actor, id, request.params).await,
            _ => JsonRpcResponse::error(id, -32601, "method not found"),
        };
        request.id.map(|_| response)
    }

    /// request本文から必要scopeを決める。未知または壊れたrequestはread scopeで認証し、その後JSON-RPCの
    /// validation errorとして返すことで、scopeの有無をoracleにしない。
    pub fn required_scope(&self, request: &JsonRpcRequest) -> &'static str {
        if request.method != "tools/call" {
            return "notes:read";
        }
        let Some(params) = &request.params else {
            return "notes:read";
        };
        let Ok(call) = serde_json::from_value::<ToolCall>(params.clone()) else {
            return "notes:read";
        };
        match call.name.as_str() {
            "create_note" | "update_note" => "notes:write",
            "prepare_delete_note" | "delete_note" => "notes:delete",
            _ => "notes:read",
        }
    }

    async fn call_tool(&self, actor: Actor, id: Value, params: Option<Value>) -> JsonRpcResponse {
        let Some(params) = params else {
            return JsonRpcResponse::error(id, -32602, "tool parameters are required");
        };
        let Ok(call) = serde_json::from_value::<ToolCall>(params) else {
            return JsonRpcResponse::error(id, -32602, "tool parameters are invalid");
        };
        match call.name.as_str() {
            "search_notes" => {
                let Ok(arguments) = serde_json::from_value::<SearchArguments>(call.arguments)
                else {
                    return JsonRpcResponse::error(id, -32602, "search arguments are invalid");
                };
                let limit = arguments.limit.unwrap_or(50).clamp(1, 100);
                let offset = match cursor_offset(arguments.cursor) {
                    Ok(offset) => offset,
                    Err(()) => {
                        return JsonRpcResponse::error(id, -32602, "search cursor is invalid");
                    }
                };
                match self
                    .notes
                    .search_notes(actor, arguments.query, offset, limit)
                    .await
                {
                    Ok(page) => {
                        let notes = page
                            .notes
                            .into_iter()
                            .map(|note| json!({ "note_id": note.note_id.to_string(), "title": note.title }))
                            .collect::<Vec<_>>();
                        let text = serde_json::to_string(&notes).expect("serializable MCP search");
                        JsonRpcResponse::success(
                            id,
                            json!({
                                "content": [{ "type": "text", "text": text }],
                                "structuredContent": {
                                    "notes": notes,
                                    "next_cursor": next_cursor(page.next_offset)
                                }
                            }),
                        )
                    }
                    Err(error) => note_error(id, error),
                }
            }
            "get_note" => {
                let Ok(arguments) = serde_json::from_value::<GetNoteArguments>(call.arguments)
                else {
                    return JsonRpcResponse::error(id, -32602, "get_note arguments are invalid");
                };
                let Ok(entity_id) = EntityId::from_str(&arguments.note_id) else {
                    return JsonRpcResponse::error(id, -32602, "note ID is invalid");
                };
                match self.notes.read_source(actor, NoteId::new(entity_id)).await {
                    Ok(note) => {
                        let note_id = note.note_id;
                        let title = note.title;
                        let revision = note.revision;
                        match String::from_utf8(note.content) {
                            Ok(source) => JsonRpcResponse::success(
                                id,
                                json!({
                                    "content": [{ "type": "text", "text": source }],
                                    "structuredContent": {
                                        "note_id": note_id.to_string(),
                                        "title": title,
                                        "revision": revision.to_hex()
                                    }
                                }),
                            ),
                            Err(_) => {
                                JsonRpcResponse::error(id, -32603, "note source is unavailable")
                            }
                        }
                    }
                    Err(error) => note_error(id, error),
                }
            }
            "create_note" => {
                let Ok(arguments) = serde_json::from_value::<NoteDraftArguments>(call.arguments)
                else {
                    return JsonRpcResponse::error(id, -32602, "create arguments are invalid");
                };
                match self.notes.create_note(actor, arguments.into()).await {
                    Ok(note) => note_source_response(id, note),
                    Err(error) => note_error(id, error),
                }
            }
            "update_note" => {
                let Ok(arguments) = serde_json::from_value::<UpdateNoteArguments>(call.arguments)
                else {
                    return JsonRpcResponse::error(id, -32602, "update arguments are invalid");
                };
                let Ok(entity_id) = EntityId::from_str(&arguments.note_id) else {
                    return JsonRpcResponse::error(id, -32602, "note ID is invalid");
                };
                let Some(revision) = SourceRevision::from_hex(&arguments.expected_revision) else {
                    return JsonRpcResponse::error(id, -32602, "expected revision is invalid");
                };
                match self
                    .notes
                    .update_note(
                        actor,
                        NoteId::new(entity_id),
                        arguments.draft.into(),
                        revision,
                    )
                    .await
                {
                    Ok(note) => note_source_response(id, note),
                    Err(error) => note_error(id, error),
                }
            }
            "prepare_delete_note" => {
                let Ok(arguments) =
                    serde_json::from_value::<PrepareDeleteArguments>(call.arguments)
                else {
                    return JsonRpcResponse::error(id, -32602, "delete arguments are invalid");
                };
                let Ok(entity_id) = EntityId::from_str(&arguments.note_id) else {
                    return JsonRpcResponse::error(id, -32602, "note ID is invalid");
                };
                let Some(revision) = SourceRevision::from_hex(&arguments.expected_revision) else {
                    return JsonRpcResponse::error(id, -32602, "expected revision is invalid");
                };
                match self
                    .notes
                    .prepare_delete_note(actor, NoteId::new(entity_id), revision)
                    .await
                {
                    Ok(preparation) => JsonRpcResponse::success(
                        id,
                        json!({
                            "content": [{ "type": "text", "text": "Delete confirmation required." }],
                            "structuredContent": {
                                "note_id": preparation.note_id.to_string(), "title": preparation.title,
                                "revision": preparation.revision.to_hex(),
                                "confirmation_token": preparation.confirmation_token
                            }
                        }),
                    ),
                    Err(error) => note_error(id, error),
                }
            }
            "delete_note" => {
                let Ok(arguments) = serde_json::from_value::<DeleteArguments>(call.arguments)
                else {
                    return JsonRpcResponse::error(id, -32602, "delete confirmation is invalid");
                };
                match self
                    .notes
                    .confirm_delete_note(actor, arguments.confirmation_token)
                    .await
                {
                    Ok(()) => JsonRpcResponse::success(
                        id,
                        json!({
                            "content": [{ "type": "text", "text": "Note deleted." }],
                            "structuredContent": { "deleted": true }
                        }),
                    ),
                    Err(error) => note_error(id, error),
                }
            }
            _ => JsonRpcResponse::error(id, -32602, "tool is not available"),
        }
    }
}

fn tool_list() -> Value {
    json!({ "tools": [
        {
            "name": "search_notes",
            "description": "Search notes visible to the authenticated user.",
            "inputSchema": { "type": "object", "required": ["query"], "properties": {
                "query": { "type": "string" }, "limit": { "type": "integer", "minimum": 1, "maximum": 100 }
            } },
            "annotations": { "readOnlyHint": true, "destructiveHint": false }
        },
        {
            "name": "create_note",
            "description": "Create a private note owned by the authenticated user.",
            "inputSchema": draft_input_schema(),
            "annotations": { "readOnlyHint": false, "destructiveHint": false }
        },
        {
            "name": "update_note",
            "description": "Update a writable note when its revision matches expected_revision.",
            "inputSchema": { "type": "object", "required": ["note_id", "expected_revision", "title", "body", "tags"], "properties": {
                "note_id": { "type": "string" }, "expected_revision": { "type": "string" },
                "title": { "type": "string" }, "body": { "type": "string" },
                "tags": { "type": "array", "items": { "type": "string" } }
            } },
            "annotations": { "readOnlyHint": false, "destructiveHint": false }
        },
        {
            "name": "prepare_delete_note",
            "description": "Prepare physical deletion and return a short-lived confirmation token.",
            "inputSchema": { "type": "object", "required": ["note_id", "expected_revision"], "properties": {
                "note_id": { "type": "string" }, "expected_revision": { "type": "string" }
            } },
            "annotations": { "readOnlyHint": false, "destructiveHint": true }
        },
        {
            "name": "delete_note",
            "description": "Physically delete a note with a single-use confirmation token.",
            "inputSchema": { "type": "object", "required": ["confirmation_token"], "properties": {
                "confirmation_token": { "type": "string" }
            } },
            "annotations": { "readOnlyHint": false, "destructiveHint": true }
        },
        {
            "name": "get_note",
            "description": "Read an AsciiDoc note visible to the authenticated user.",
            "inputSchema": { "type": "object", "required": ["note_id"], "properties": {
                "note_id": { "type": "string" }
            } },
            "annotations": { "readOnlyHint": true, "destructiveHint": false }
        }
    ] })
}

fn draft_input_schema() -> Value {
    json!({ "type": "object", "required": ["title", "body", "tags"], "properties": {
        "title": { "type": "string" }, "body": { "type": "string" },
        "tags": { "type": "array", "items": { "type": "string" } }
    } })
}

fn note_source_response(id: Value, note: marginalis_domain::NoteSource) -> JsonRpcResponse {
    let note_id = note.note_id;
    let title = note.title;
    let revision = note.revision;
    let Ok(source) = String::from_utf8(note.content) else {
        return JsonRpcResponse::error(id, -32603, "note source is unavailable");
    };
    JsonRpcResponse::success(
        id,
        json!({
            "content": [{ "type": "text", "text": source }],
            "structuredContent": {
                "note_id": note_id.to_string(), "title": title, "revision": revision.to_hex()
            }
        }),
    )
}

fn note_error(id: Value, error: NoteUseCaseError) -> JsonRpcResponse {
    match error {
        NoteUseCaseError::NotFound | NoteUseCaseError::Forbidden => {
            JsonRpcResponse::error(id, -32004, "note is not available")
        }
        NoteUseCaseError::Validation => JsonRpcResponse::error(id, -32602, "request is invalid"),
        NoteUseCaseError::Conflict => {
            JsonRpcResponse::error(id, -32009, "note operation conflicts")
        }
        NoteUseCaseError::Unavailable => {
            JsonRpcResponse::error(id, -32603, "service is unavailable")
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct JsonRpcRequest {
    #[serde(default = "json_rpc_version")]
    pub jsonrpc: String,
    pub id: Option<Value>,
    pub method: String,
    pub params: Option<Value>,
}

fn json_rpc_version() -> String {
    "2.0".into()
}

#[derive(Clone, Debug, Serialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: &'static str,
    pub id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

#[derive(Clone, Debug, Serialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: &'static str,
}

impl JsonRpcResponse {
    fn success(id: Value, result: Value) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: Some(result),
            error: None,
        }
    }
    fn error(id: Value, code: i32, message: &'static str) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(JsonRpcError { code, message }),
        }
    }
}

#[derive(Deserialize)]
struct ToolCall {
    name: String,
    arguments: Value,
}
#[derive(Deserialize)]
struct SearchArguments {
    query: String,
    limit: Option<u32>,
    cursor: Option<String>,
}
#[derive(Deserialize)]
struct GetNoteArguments {
    note_id: String,
}

#[derive(Deserialize)]
struct NoteDraftArguments {
    title: String,
    body: String,
    tags: Vec<String>,
}

impl From<NoteDraftArguments> for NoteDraft {
    fn from(value: NoteDraftArguments) -> Self {
        Self {
            title: value.title,
            body: value.body,
            tags: value.tags,
        }
    }
}

#[derive(Deserialize)]
struct UpdateNoteArguments {
    note_id: String,
    expected_revision: String,
    #[serde(flatten)]
    draft: NoteDraftArguments,
}

#[derive(Deserialize)]
struct PrepareDeleteArguments {
    note_id: String,
    expected_revision: String,
}

#[derive(Deserialize)]
struct DeleteArguments {
    confirmation_token: String,
}

fn cursor_offset(cursor: Option<String>) -> Result<u64, ()> {
    let Some(cursor) = cursor else {
        return Ok(0);
    };
    let bytes = URL_SAFE_NO_PAD.decode(cursor).map_err(|_| ())?;
    let bytes: [u8; 8] = bytes.try_into().map_err(|_| ())?;
    Ok(u64::from_be_bytes(bytes))
}

fn next_cursor(offset: Option<u64>) -> Option<String> {
    offset.map(|offset| URL_SAFE_NO_PAD.encode(offset.to_be_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use marginalis_application::{DeletePreparation, NoteUseCaseError};
    use marginalis_domain::{NotePage, NotePermission, NoteSource, SourceRevision, UserId};

    struct EmptyNotes;

    #[async_trait]
    impl NoteUseCases for EmptyNotes {
        async fn list_notes(
            &self,
            _actor: Actor,
            _offset: u64,
            _limit: u32,
        ) -> Result<NotePage, NoteUseCaseError> {
            Ok(NotePage {
                notes: Vec::new(),
                next_offset: None,
            })
        }
        async fn search_notes(
            &self,
            _actor: Actor,
            _query: String,
            _offset: u64,
            _limit: u32,
        ) -> Result<NotePage, NoteUseCaseError> {
            Ok(NotePage {
                notes: Vec::new(),
                next_offset: None,
            })
        }
        async fn read_source(
            &self,
            _actor: Actor,
            _note_id: NoteId,
        ) -> Result<NoteSource, NoteUseCaseError> {
            Err(NoteUseCaseError::NotFound)
        }
        async fn create_source(
            &self,
            _actor: Actor,
            _source: String,
        ) -> Result<NoteId, NoteUseCaseError> {
            Err(NoteUseCaseError::Unavailable)
        }
        async fn create_note(
            &self,
            _actor: Actor,
            _draft: NoteDraft,
        ) -> Result<NoteSource, NoteUseCaseError> {
            Ok(NoteSource {
                note_id: note_id(),
                title: "Created".into(),
                revision: SourceRevision::from_source(b"= Created\n"),
                content: b"= Created\n".to_vec(),
            })
        }
        async fn update_source(
            &self,
            _actor: Actor,
            _note_id: NoteId,
            _source: String,
            _expected_revision: SourceRevision,
        ) -> Result<(), NoteUseCaseError> {
            Err(NoteUseCaseError::Unavailable)
        }
        async fn update_note(
            &self,
            _actor: Actor,
            _note_id: NoteId,
            _draft: NoteDraft,
            _expected_revision: SourceRevision,
        ) -> Result<NoteSource, NoteUseCaseError> {
            Err(NoteUseCaseError::Unavailable)
        }
        async fn delete_note(
            &self,
            _actor: Actor,
            _note_id: NoteId,
            _expected_revision: SourceRevision,
        ) -> Result<(), NoteUseCaseError> {
            Err(NoteUseCaseError::Unavailable)
        }
        async fn prepare_delete_note(
            &self,
            _actor: Actor,
            _note_id: NoteId,
            _expected_revision: SourceRevision,
        ) -> Result<DeletePreparation, NoteUseCaseError> {
            Err(NoteUseCaseError::Unavailable)
        }
        async fn confirm_delete_note(
            &self,
            _actor: Actor,
            _confirmation_token: String,
        ) -> Result<(), NoteUseCaseError> {
            Err(NoteUseCaseError::Unavailable)
        }
        async fn set_permission(
            &self,
            _actor: Actor,
            _note_id: NoteId,
            _user_id: UserId,
            _permission: Option<NotePermission>,
        ) -> Result<(), NoteUseCaseError> {
            Err(NoteUseCaseError::Unavailable)
        }
    }

    fn actor() -> Actor {
        Actor {
            user_id: UserId::new(
                EntityId::from_str("01800000-0000-7000-8000-000000000081").expect("user"),
            ),
            is_root: false,
        }
    }

    fn note_id() -> NoteId {
        NoteId::new(EntityId::from_str("01800000-0000-7000-8000-000000000082").expect("note"))
    }

    #[tokio::test]
    async fn initialize_and_tool_list_follow_json_rpc() {
        let tools = McpTools::new(Arc::new(EmptyNotes));
        let response = tools
            .handle(
                actor(),
                JsonRpcRequest {
                    jsonrpc: "2.0".into(),
                    id: Some(json!(1)),
                    method: "initialize".into(),
                    params: None,
                },
            )
            .await
            .expect("response");
        assert_eq!(
            response.result.expect("result")["protocolVersion"],
            MCP_PROTOCOL_VERSION
        );
        let response = tools
            .handle(
                actor(),
                JsonRpcRequest {
                    jsonrpc: "2.0".into(),
                    id: Some(json!(2)),
                    method: "tools/list".into(),
                    params: None,
                },
            )
            .await
            .expect("response");
        assert_eq!(
            response.result.expect("result")["tools"]
                .as_array()
                .expect("tools")
                .len(),
            6
        );
    }

    #[tokio::test]
    async fn notification_has_no_json_rpc_response() {
        let tools = McpTools::new(Arc::new(EmptyNotes));
        assert!(
            tools
                .handle(
                    actor(),
                    JsonRpcRequest {
                        jsonrpc: "2.0".into(),
                        id: None,
                        method: "tools/list".into(),
                        params: None,
                    }
                )
                .await
                .is_none()
        );
    }

    #[tokio::test]
    async fn create_note_uses_write_scope_and_returns_server_metadata() {
        let tools = McpTools::new(Arc::new(EmptyNotes));
        let request = JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(3)),
            method: "tools/call".into(),
            params: Some(json!({
                "name": "create_note",
                "arguments": { "title": "Created", "body": "text", "tags": ["research"] }
            })),
        };
        assert_eq!(tools.required_scope(&request), "notes:write");
        let response = tools.handle(actor(), request).await.expect("response");
        assert_eq!(
            response.result.expect("result")["structuredContent"]["note_id"],
            note_id().to_string()
        );
    }
}
