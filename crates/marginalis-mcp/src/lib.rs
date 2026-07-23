//! MCP JSON-RPCのtool adapter。HTTP、OAuthおよびSQLiteには依存しない。

use async_trait::async_trait;
use marginalis_application::{NoteUseCaseError, NoteUseCases};
use marginalis_domain::{Actor, EntityId, NoteId};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{str::FromStr, sync::Arc};

pub const MCP_PROTOCOL_VERSION: &str = "2025-11-25";

/// bearer tokenを検証済みの一般ユーザーへ変換するMCP専用の境界。
#[async_trait]
pub trait McpAuthenticator: Send + Sync {
    async fn authenticate_read(&self, bearer_token: &str) -> Result<Actor, McpAuthenticationError>;
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

    pub async fn handle(&self, actor: Actor, request: JsonRpcRequest) -> JsonRpcResponse {
        let id = request.id.unwrap_or(Value::Null);
        match request.method.as_str() {
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
                match self
                    .notes
                    .search_notes(actor, arguments.query, 0, limit)
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
                                "structuredContent": { "notes": notes }
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
                    Ok(source) => match String::from_utf8(source.content) {
                        Ok(source) => JsonRpcResponse::success(
                            id,
                            json!({ "content": [{ "type": "text", "text": source }] }),
                        ),
                        Err(_) => JsonRpcResponse::error(id, -32603, "note source is unavailable"),
                    },
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
            "name": "get_note",
            "description": "Read an AsciiDoc note visible to the authenticated user.",
            "inputSchema": { "type": "object", "required": ["note_id"], "properties": {
                "note_id": { "type": "string" }
            } },
            "annotations": { "readOnlyHint": true, "destructiveHint": false }
        }
    ] })
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
}
#[derive(Deserialize)]
struct GetNoteArguments {
    note_id: String,
}
