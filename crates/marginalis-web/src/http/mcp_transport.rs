//! MCP Streamable HTTPとJSON-RPC tool dispatch。

use super::*;

#[derive(Deserialize)]
struct McpToolCall {
    name: String,
    arguments: serde_json::Value,
}

fn mcp_required_scope(request: &JsonRpcRequest) -> &'static str {
    if request.method != "tools/call" {
        return "notes:read";
    }
    serde_json::from_value::<McpToolCall>(request.params.clone().unwrap_or_default())
        .ok()
        .map(|call| match call.name.as_str() {
            "create_note" | "update_note" => "notes:write",
            "delete_note" => "notes:delete",
            _ => "notes:read",
        })
        .unwrap_or("notes:read")
}

fn mcp_unauthorized(endpoint: &McpEndpoint) -> Response {
    let mut response = StatusCode::UNAUTHORIZED.into_response();
    if let Ok(value) = format!("Bearer resource_metadata=\"{}\"", endpoint.metadata_uri).parse() {
        response
            .headers_mut()
            .insert(header::WWW_AUTHENTICATE, value);
    }
    response
}

pub(super) async fn mcp_post(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(request): Json<JsonRpcRequest>,
) -> HandlerResult<Response> {
    let endpoint = mcp_endpoint(&state)?;
    if let Some(value) = headers.get(header::ORIGIN) {
        let origin = value.to_str().map_err(|_| {
            problem(
                StatusCode::FORBIDDEN,
                "origin_not_allowed",
                "MCP browser request origin is not allowed",
            )
        })?;
        if origin != state.browser_origin
            && !endpoint
                .allowed_origins
                .iter()
                .any(|allowed| allowed == origin)
        {
            tracing::warn!(
                received_origin = origin,
                "rejected MCP browser request from an untrusted origin"
            );
            return Err(problem(
                StatusCode::FORBIDDEN,
                "origin_not_allowed",
                "MCP browser request origin is not allowed",
            ));
        }
    }
    let accepts = headers
        .get(header::ACCEPT)
        .and_then(|value| value.to_str().ok())
        .map(|value| {
            value
                .split(',')
                .map(|item| item.trim().split(';').next().unwrap_or_default())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if !accepts.contains(&"application/json") || !accepts.contains(&"text/event-stream") {
        return Ok(StatusCode::NOT_ACCEPTABLE.into_response());
    }
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));
    let Some(token) = token else {
        return Ok(mcp_unauthorized(endpoint));
    };
    let Some(actor) = endpoint
        .oauth
        .authenticate(
            token.into(),
            endpoint.resource_uri.clone(),
            mcp_required_scope(&request).into(),
        )
        .await
        .map_err(mcp_error)?
        .map(|value| value.actor)
    else {
        return Ok(mcp_unauthorized(endpoint));
    };
    let id = request.id.clone().unwrap_or(serde_json::Value::Null);
    if request.jsonrpc != "2.0" {
        return if request.id.is_none() {
            Ok(StatusCode::ACCEPTED.into_response())
        } else {
            Ok(Json(JsonRpcResponse::error(
                id,
                -32600,
                "JSON-RPC version is invalid",
            ))
            .into_response())
        };
    }
    let response = match request.method.as_str() {
        "initialize" => JsonRpcResponse::success(
            id,
            serde_json::json!({"protocolVersion": MCP_PROTOCOL_VERSION, "capabilities":{"tools":{}}, "serverInfo":{"name":"marginalis","version":env!("CARGO_PKG_VERSION")}}),
        ),
        "tools/list" => JsonRpcResponse::success(
            id,
            serde_json::json!({"tools":[
                {"name":"list_notes","description":"List notes visible to the authenticated user.","inputSchema":{"type":"object","properties":{}}},
                {"name":"get_note","description":"Read one visible note.","inputSchema":{"type":"object","required":["note_id"],"properties":{"note_id":{"type":"string"}}}},
                {"name":"create_note","description":"Create a note.","inputSchema":{"type":"object","required":["title","body","tags"],"properties":{"title":{"type":"string"},"body":{"type":"string"},"tags":{"type":"array","items":{"type":"string"}}}}},
                {"name":"update_note","description":"Update a note at its current revision.","inputSchema":{"type":"object","required":["note_id","title","body","tags","expected_revision"],"properties":{"note_id":{"type":"string"},"title":{"type":"string"},"body":{"type":"string"},"tags":{"type":"array","items":{"type":"string"}},"expected_revision":{"type":"integer"}}}},
                {"name":"delete_note","description":"Soft-delete a note at its current revision.","inputSchema":{"type":"object","required":["note_id","expected_revision"],"properties":{"note_id":{"type":"string"},"expected_revision":{"type":"integer"}}}}
            ]}),
        ),
        "tools/call" => mcp_tool_call(endpoint, actor, id, request.params).await,
        _ => JsonRpcResponse::error(id, -32601, "method not found"),
    };
    if request.id.is_none() {
        Ok(StatusCode::ACCEPTED.into_response())
    } else {
        Ok(Json(response).into_response())
    }
}

#[derive(Deserialize)]
struct McpUpdate {
    note_id: String,
    title: String,
    body: String,
    tags: Vec<String>,
    expected_revision: i64,
}

#[derive(Deserialize)]
struct McpDelete {
    note_id: String,
    expected_revision: i64,
}

async fn mcp_tool_call(
    endpoint: &McpEndpoint,
    actor: Actor,
    id: serde_json::Value,
    params: Option<serde_json::Value>,
) -> JsonRpcResponse {
    let Ok(call) = serde_json::from_value::<McpToolCall>(params.unwrap_or_default()) else {
        return JsonRpcResponse::error(id, -32602, "tool parameters are invalid");
    };
    let result = match call.name.as_str() {
        "list_notes" => endpoint.notes.list_visible_notes(actor).await.map(|notes| {
            serde_json::json!(
                notes
                    .into_iter()
                    .map(|note| serde_json::json!({
                        "note_id": note.note_id.to_string(),
                        "title": note.title,
                        "revision": note.revision,
                    }))
                    .collect::<Vec<_>>()
            )
        }),
        "get_note" => {
            let Some(note_id) = mcp_note_id(&call.arguments) else {
                return JsonRpcResponse::error(id, -32602, "note_id is invalid");
            };
            endpoint.notes.read_note(actor, note_id).await.map(|note| {
                serde_json::json!({
                    "note_id": note.note_id.to_string(),
                    "title": note.title,
                    "body": note.body,
                    "tags": note.tags,
                    "revision": note.revision,
                })
            })
        }
        "create_note" => {
            let Ok(input) = serde_json::from_value::<NoteInput>(call.arguments) else {
                return JsonRpcResponse::error(id, -32602, "note arguments are invalid");
            };
            endpoint
                .notes
                .create_note(
                    actor,
                    NoteDraft {
                        title: input.title,
                        body: input.body,
                        tags: input.tags,
                    },
                )
                .await
                .map(note_revision_json)
        }
        "update_note" => {
            let Ok(input) = serde_json::from_value::<McpUpdate>(call.arguments) else {
                return JsonRpcResponse::error(id, -32602, "update arguments are invalid");
            };
            let Some(note_id) = parse_note_id(&input.note_id).ok() else {
                return JsonRpcResponse::error(id, -32602, "note_id is invalid");
            };
            endpoint
                .notes
                .update_note(
                    actor,
                    note_id,
                    NoteDraft {
                        title: input.title,
                        body: input.body,
                        tags: input.tags,
                    },
                    input.expected_revision,
                )
                .await
                .map(note_revision_json)
        }
        "delete_note" => {
            let Ok(input) = serde_json::from_value::<McpDelete>(call.arguments) else {
                return JsonRpcResponse::error(id, -32602, "delete arguments are invalid");
            };
            let Some(note_id) = parse_note_id(&input.note_id).ok() else {
                return JsonRpcResponse::error(id, -32602, "note_id is invalid");
            };
            endpoint
                .notes
                .soft_delete_note(actor, note_id, input.expected_revision)
                .await
                .map(note_revision_json)
        }
        _ => return JsonRpcResponse::error(id, -32601, "tool not found"),
    };
    match result {
        Ok(value) => JsonRpcResponse::success(
            id,
            serde_json::json!({"content":[{"type":"text","text":serde_json::to_string(&value).unwrap_or_default()}],"structuredContent":value}),
        ),
        Err(_) => JsonRpcResponse::error(id, -32000, "note operation failed"),
    }
}

fn mcp_note_id(arguments: &serde_json::Value) -> Option<NoteId> {
    arguments
        .get("note_id")
        .and_then(serde_json::Value::as_str)
        .and_then(|value| parse_note_id(value).ok())
}

fn note_revision_json(note: Note) -> serde_json::Value {
    serde_json::json!({
        "note_id": note.note_id.to_string(),
        "revision": note.revision,
    })
}
