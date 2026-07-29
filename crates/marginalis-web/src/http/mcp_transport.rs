//! MCP Streamable HTTPとJSON-RPC tool dispatch。

use axum::{
    Json,
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use marginalis_application::{
    McpAccessTokenAuthenticationError, NoteProfile, NoteUseCaseError, NoteUseCases,
};
use marginalis_contract::ProblemCode;
use marginalis_domain::{Actor, Note, NoteDraft, Revision};
use serde::Deserialize;

use crate::mcp::{JsonRpcRequest, JsonRpcResponse, MCP_PROTOCOL_VERSION};

use super::{
    auth::parse_note_id,
    error::{HandlerResult, problem, validation_problem_json},
    mcp_endpoint,
    notes::NoteInput,
    state::{ApiState, McpEndpoint},
};

pub(super) async fn mcp_resource_metadata(
    State(state): State<ApiState>,
) -> HandlerResult<Json<serde_json::Value>> {
    let endpoint = mcp_endpoint(&state)?;
    Ok(Json(serde_json::json!({
        "resource": endpoint.resource_uri,
        "resource_name": "Marginalis MCP",
        "authorization_servers": [endpoint.authorization_server_uri],
        "bearer_methods_supported": ["header"],
        "scopes_supported": ["notes:read", "notes:write", "notes:delete"]
    })))
}

struct McpToolCall {
    tool: McpTool,
    arguments: serde_json::Value,
}

#[derive(Clone, Copy)]
enum McpTool {
    ListNotes,
    GetNoteProfile,
    GetNote,
    CreateNote,
    UpdateNote,
    DeleteNote,
    Unknown,
}

impl McpTool {
    fn from_name(name: &str) -> Self {
        match name {
            "list_notes" => Self::ListNotes,
            "get_note_profile" => Self::GetNoteProfile,
            "get_note" => Self::GetNote,
            "create_note" => Self::CreateNote,
            "update_note" => Self::UpdateNote,
            "delete_note" => Self::DeleteNote,
            _ => Self::Unknown,
        }
    }

    fn accepted_scopes(self) -> &'static [&'static str] {
        match self {
            Self::ListNotes | Self::GetNote => &["notes:read"],
            Self::GetNoteProfile => &["notes:read", "notes:write"],
            Self::CreateNote | Self::UpdateNote => &["notes:write"],
            Self::DeleteNote => &["notes:delete"],
            Self::Unknown => &[],
        }
    }
}

#[derive(Deserialize)]
struct RawMcpToolCall {
    name: String,
    #[serde(default = "empty_json_object")]
    arguments: serde_json::Value,
}

fn empty_json_object() -> serde_json::Value {
    serde_json::json!({})
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct McpInitialize {
    protocol_version: String,
    capabilities: serde_json::Value,
    client_info: McpClientInfo,
}

#[derive(Deserialize)]
struct McpClientInfo {
    name: String,
    version: String,
}

fn decode_tool_call(params: Option<serde_json::Value>) -> Result<McpToolCall, ()> {
    let raw =
        serde_json::from_value::<RawMcpToolCall>(params.unwrap_or_default()).map_err(|_| ())?;
    if !raw.arguments.is_object() {
        return Err(());
    }
    Ok(McpToolCall {
        tool: McpTool::from_name(&raw.name),
        arguments: raw.arguments,
    })
}

fn initialize_result(params: Option<serde_json::Value>) -> Result<serde_json::Value, ()> {
    let initialization =
        serde_json::from_value::<McpInitialize>(params.unwrap_or_default()).map_err(|_| ())?;
    if !valid_client_capabilities(&initialization.capabilities)
        || initialization.client_info.name.trim().is_empty()
        || initialization.client_info.version.trim().is_empty()
    {
        return Err(());
    }
    let protocol_version = if matches!(
        initialization.protocol_version.as_str(),
        MCP_PROTOCOL_VERSION | "2025-03-26"
    ) {
        initialization.protocol_version
    } else {
        MCP_PROTOCOL_VERSION.into()
    };
    Ok(serde_json::json!({
        "protocolVersion": protocol_version,
        "capabilities":{"tools":{}},
        "serverInfo":{"name":"marginalis","version":env!("CARGO_PKG_VERSION")}
    }))
}

fn valid_client_capabilities(capabilities: &serde_json::Value) -> bool {
    let Some(capabilities) = capabilities.as_object() else {
        return false;
    };
    for name in ["experimental", "sampling", "elicitation"] {
        if capabilities
            .get(name)
            .is_some_and(|value| !value.is_object())
        {
            return false;
        }
    }
    capabilities.get("roots").is_none_or(|roots| {
        roots.as_object().is_some_and(|roots| {
            roots
                .get("listChanged")
                .is_none_or(serde_json::Value::is_boolean)
        })
    })
}

fn valid_tools_list_params(params: Option<&serde_json::Value>) -> bool {
    params.is_none_or(|params| {
        params
            .as_object()
            .is_some_and(|params| !params.contains_key("cursor"))
    })
}

fn mcp_authentication_error(
    endpoint: &McpEndpoint,
    status: StatusCode,
    error: Option<&str>,
    scope: &str,
) -> Response {
    let mut response = status.into_response();
    let error = error.map_or_else(String::new, |value| format!(", error=\"{value}\""));
    if let Ok(value) = format!(
        "Bearer resource_metadata=\"{}\", scope=\"{}\"{}",
        endpoint.metadata_uri, scope, error
    )
    .parse()
    {
        response
            .headers_mut()
            .insert(header::WWW_AUTHENTICATE, value);
    }
    response
}

fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let mut parts = value.split_ascii_whitespace();
    let scheme = parts.next()?;
    let token = parts.next()?;
    (scheme.eq_ignore_ascii_case("bearer") && !token.is_empty() && parts.next().is_none())
        .then_some(token)
}

fn accepts_media_type(headers: &HeaderMap, expected: &str) -> bool {
    headers.get_all(header::ACCEPT).iter().any(|value| {
        value.to_str().ok().is_some_and(|value| {
            value.split(',').any(|item| {
                let mut parts = item.split(';');
                let media_type = parts.next().unwrap_or_default().trim();
                let refused = parts.any(|parameter| {
                    parameter
                        .trim()
                        .split_once('=')
                        .is_some_and(|(name, value)| {
                            name.eq_ignore_ascii_case("q")
                                && value
                                    .trim()
                                    .parse::<f32>()
                                    .is_ok_and(|quality| quality == 0.0)
                        })
                });
                media_type.eq_ignore_ascii_case(expected) && !refused
            })
        })
    })
}

fn validate_mcp_origin(
    state: &ApiState,
    endpoint: &McpEndpoint,
    headers: &HeaderMap,
) -> HandlerResult<()> {
    let Some(value) = headers.get(header::ORIGIN) else {
        return Ok(());
    };
    let origin = value.to_str().map_err(|_| {
        problem(
            StatusCode::FORBIDDEN,
            ProblemCode::OriginNotAllowed,
            "MCP browser request origin is not allowed",
        )
    })?;
    if origin == state.browser_origin
        || endpoint
            .allowed_origins
            .iter()
            .any(|allowed| allowed == origin)
    {
        return Ok(());
    }
    tracing::warn!(
        received_origin = origin,
        "rejected MCP browser request from an untrusted origin"
    );
    Err(problem(
        StatusCode::FORBIDDEN,
        ProblemCode::OriginNotAllowed,
        "MCP browser request origin is not allowed",
    ))
}

pub(super) async fn mcp_unsupported_method(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> HandlerResult<Response> {
    let endpoint = mcp_endpoint(&state)?;
    validate_mcp_origin(&state, endpoint, &headers)?;
    Ok(StatusCode::METHOD_NOT_ALLOWED.into_response())
}

pub(super) async fn mcp_post(
    State(state): State<ApiState>,
    headers: HeaderMap,
    body: Bytes,
) -> HandlerResult<Response> {
    let endpoint = mcp_endpoint(&state)?;
    validate_mcp_origin(&state, endpoint, &headers)?;
    if bearer_token(&headers).is_none() {
        return Ok(mcp_authentication_error(
            endpoint,
            StatusCode::UNAUTHORIZED,
            None,
            "notes:read",
        ));
    }
    if !accepts_media_type(&headers, "application/json")
        || !accepts_media_type(&headers, "text/event-stream")
    {
        return Ok(StatusCode::NOT_ACCEPTABLE.into_response());
    }
    let content_type_is_json = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .is_some_and(|value| value.trim().eq_ignore_ascii_case("application/json"));
    if !content_type_is_json {
        return Ok(StatusCode::UNSUPPORTED_MEDIA_TYPE.into_response());
    }
    let request_value = match serde_json::from_slice::<serde_json::Value>(&body) {
        Ok(value) => value,
        Err(_) => {
            return Ok(Json(JsonRpcResponse::error(
                serde_json::Value::Null,
                -32700,
                "Parse error",
            ))
            .into_response());
        }
    };
    if request_value.as_object().is_some_and(|object| {
        !object.contains_key("method")
            && (object.contains_key("result") || object.contains_key("error"))
    }) {
        // This server never sends client-bound requests, so it cannot accept a client response.
        return Ok(StatusCode::BAD_REQUEST.into_response());
    }
    let invalid_request_id = detected_request_id(&request_value);
    let request = match serde_json::from_value::<JsonRpcRequest>(request_value) {
        Ok(request) => request,
        Err(_) => {
            return Ok(Json(JsonRpcResponse::error(
                invalid_request_id,
                -32600,
                "Invalid Request",
            ))
            .into_response());
        }
    };
    if request.method != "initialize" {
        let protocol_version = headers
            .get("mcp-protocol-version")
            .map(|value| value.to_str().ok())
            .unwrap_or(Some("2025-03-26"));
        if !matches!(protocol_version, Some(MCP_PROTOCOL_VERSION | "2025-03-26")) {
            return Ok(StatusCode::BAD_REQUEST.into_response());
        }
    }
    let decoded_tool_call =
        (request.method == "tools/call").then(|| decode_tool_call(request.params.clone()));
    let required_scope = decoded_tool_call
        .as_ref()
        .and_then(|call| call.as_ref().ok())
        .map_or(&[][..], |call| call.tool.accepted_scopes());
    let challenged_scope = required_scope.first().copied().unwrap_or("notes:read");
    let token = bearer_token(&headers);
    let Some(token) = token else {
        return Ok(mcp_authentication_error(
            endpoint,
            StatusCode::UNAUTHORIZED,
            None,
            challenged_scope,
        ));
    };
    let authenticated = match endpoint
        .access_token_authenticator
        .authenticate_access_token(token.into(), endpoint.resource_uri.clone())
        .await
    {
        Ok(authenticated) => authenticated,
        Err(McpAccessTokenAuthenticationError::Rejected(reason)) => {
            tracing::warn!(
                event = "mcp.authentication.failed",
                reason = reason.log_reason(),
                "MCP access token was rejected"
            );
            return Ok(mcp_authentication_error(
                endpoint,
                StatusCode::UNAUTHORIZED,
                Some("invalid_token"),
                challenged_scope,
            ));
        }
        Err(error) => {
            let reason = match error {
                McpAccessTokenAuthenticationError::Configuration => "configuration",
                McpAccessTokenAuthenticationError::Discovery => "discovery",
                McpAccessTokenAuthenticationError::Unavailable => "upstream-unavailable",
                McpAccessTokenAuthenticationError::Rejected(_) => unreachable!(),
            };
            tracing::error!(
                event = "mcp.authentication.unavailable",
                reason,
                "MCP access token authentication is unavailable"
            );
            return Err(problem(
                StatusCode::SERVICE_UNAVAILABLE,
                ProblemCode::Unavailable,
                "MCP authentication is unavailable",
            ));
        }
    };
    let Some(authenticated) = authenticated else {
        tracing::warn!(
            event = "mcp.authentication.failed",
            reason = "invalid-token",
            "MCP access token was rejected"
        );
        return Ok(mcp_authentication_error(
            endpoint,
            StatusCode::UNAUTHORIZED,
            Some("invalid_token"),
            challenged_scope,
        ));
    };
    if !required_scope.is_empty()
        && !required_scope
            .iter()
            .any(|required| authenticated.scopes.iter().any(|scope| scope == required))
    {
        tracing::warn!(
            event = "mcp.authorization.failed",
            reason = "insufficient-scope",
            required_scope = challenged_scope,
            "MCP access token has insufficient scope"
        );
        return Ok(mcp_authentication_error(
            endpoint,
            StatusCode::FORBIDDEN,
            Some("insufficient_scope"),
            challenged_scope,
        ));
    }
    let actor = authenticated.actor;
    let id = request.id.response_value();
    let params_are_valid = request
        .params
        .as_ref()
        .is_none_or(serde_json::Value::is_object);
    if request.jsonrpc.as_deref() != Some("2.0")
        || !request.id.is_valid_mcp_id()
        || !params_are_valid
    {
        if request.id.is_notification() && request.jsonrpc.as_deref() == Some("2.0") {
            return Ok(StatusCode::BAD_REQUEST.into_response());
        }
        let error_id = if request.id.is_valid_mcp_id() {
            id
        } else {
            serde_json::Value::Null
        };
        return Ok(
            Json(JsonRpcResponse::error(error_id, -32600, "Invalid Request")).into_response(),
        );
    }
    let request_requires_id = matches!(
        request.method.as_str(),
        "initialize" | "ping" | "tools/list" | "tools/call"
    );
    if request_requires_id && request.id.is_notification() {
        return Ok(StatusCode::BAD_REQUEST.into_response());
    }
    if request.method == "notifications/initialized" && !request.id.is_notification() {
        return Ok(Json(JsonRpcResponse::error(id, -32600, "Invalid Request")).into_response());
    }
    let response = match request.method.as_str() {
        "initialize" => match initialize_result(request.params) {
            Ok(result) => JsonRpcResponse::success(id, result),
            Err(()) => JsonRpcResponse::error(id, -32602, "Invalid params"),
        },
        "notifications/initialized" => JsonRpcResponse::success(id, serde_json::json!({})),
        "ping" => JsonRpcResponse::success(id, serde_json::json!({})),
        "tools/list" if valid_tools_list_params(request.params.as_ref()) => {
            JsonRpcResponse::success(
                id,
                serde_json::json!({"tools": marginalis_contract::mcp_tool_contracts()}),
            )
        }
        "tools/list" => JsonRpcResponse::error(id, -32602, "Invalid params"),
        "tools/call" => match decoded_tool_call.expect("tools/call was decoded") {
            Ok(call) => mcp_tool_call(state.notes.as_ref(), actor, id, call).await,
            Err(()) => JsonRpcResponse::error(id, -32602, "Invalid params"),
        },
        _ => JsonRpcResponse::error(id, -32601, "Method not found"),
    };
    if request.id.is_notification() {
        Ok(StatusCode::ACCEPTED.into_response())
    } else {
        Ok(Json(response).into_response())
    }
}

fn detected_request_id(value: &serde_json::Value) -> serde_json::Value {
    value
        .as_object()
        .and_then(|object| object.get("id"))
        .filter(|id| match id {
            serde_json::Value::String(_) => true,
            serde_json::Value::Number(number) => {
                number.as_i64().is_some() || number.as_u64().is_some()
            }
            _ => false,
        })
        .cloned()
        .unwrap_or(serde_json::Value::Null)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct McpGet {
    note_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct McpUpdate {
    note_id: String,
    source: String,
    expected_revision: i64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct McpDelete {
    note_id: String,
    expected_revision: i64,
}

async fn mcp_tool_call(
    notes: &dyn NoteUseCases,
    actor: Actor,
    id: serde_json::Value,
    call: McpToolCall,
) -> JsonRpcResponse {
    let result = match call.tool {
        McpTool::ListNotes
            if call
                .arguments
                .as_object()
                .is_none_or(|value| !value.is_empty()) =>
        {
            return JsonRpcResponse::error(id, -32602, "list arguments are invalid");
        }
        McpTool::ListNotes => notes.list_visible_notes(actor).await.map(|notes| {
            serde_json::json!({
                "notes": notes
                    .into_iter()
                    .map(|entry| serde_json::json!({
                        "note_id": entry.summary.note_id.to_string(),
                        "title": entry.summary.title,
                        "revision": entry.summary.revision.get(),
                    }))
                    .collect::<Vec<_>>()
            })
        }),
        McpTool::GetNoteProfile
            if call
                .arguments
                .as_object()
                .is_none_or(|value| !value.is_empty()) =>
        {
            return JsonRpcResponse::error(id, -32602, "profile arguments are invalid");
        }
        McpTool::GetNoteProfile => Ok(note_profile_json(notes.note_profile())),
        McpTool::GetNote => {
            let Ok(input) = serde_json::from_value::<McpGet>(call.arguments) else {
                return JsonRpcResponse::error(id, -32602, "get arguments are invalid");
            };
            let Some(note_id) = parse_note_id(&input.note_id).ok() else {
                return JsonRpcResponse::error(id, -32602, "note_id is invalid");
            };
            notes.read_note(actor, note_id).await.map(|note| {
                serde_json::json!({
                    "note_id": note.note_id().to_string(),
                    "title": note.title(),
                    "source": note.source(),
                    "tags": note.tags(),
                    "revision": note.revision().get(),
                })
            })
        }
        McpTool::CreateNote => {
            let Ok(input) = serde_json::from_value::<NoteInput>(call.arguments) else {
                return JsonRpcResponse::error(id, -32602, "note arguments are invalid");
            };
            notes
                .create_note(
                    actor,
                    NoteDraft {
                        source: input.source,
                        title: String::new(),
                        tags: Vec::new(),
                    },
                )
                .await
                .map(note_revision_json)
        }
        McpTool::UpdateNote => {
            let Ok(input) = serde_json::from_value::<McpUpdate>(call.arguments) else {
                return JsonRpcResponse::error(id, -32602, "update arguments are invalid");
            };
            let Some(note_id) = parse_note_id(&input.note_id).ok() else {
                return JsonRpcResponse::error(id, -32602, "note_id is invalid");
            };
            let Ok(expected_revision) = Revision::new(input.expected_revision) else {
                return JsonRpcResponse::error(id, -32602, "expected_revision is invalid");
            };
            notes
                .update_note(
                    actor,
                    note_id,
                    NoteDraft {
                        source: input.source,
                        title: String::new(),
                        tags: Vec::new(),
                    },
                    expected_revision,
                )
                .await
                .map(note_revision_json)
        }
        McpTool::DeleteNote => {
            let Ok(input) = serde_json::from_value::<McpDelete>(call.arguments) else {
                return JsonRpcResponse::error(id, -32602, "delete arguments are invalid");
            };
            let Some(note_id) = parse_note_id(&input.note_id).ok() else {
                return JsonRpcResponse::error(id, -32602, "note_id is invalid");
            };
            let Ok(expected_revision) = Revision::new(input.expected_revision) else {
                return JsonRpcResponse::error(id, -32602, "expected_revision is invalid");
            };
            notes
                .soft_delete_note(actor, note_id, expected_revision)
                .await
                .map(note_revision_json)
        }
        McpTool::Unknown => return JsonRpcResponse::error(id, -32602, "Unknown tool"),
    };
    match result {
        Ok(value) => JsonRpcResponse::success(
            id,
            serde_json::json!({"content":[{"type":"text","text":serde_json::to_string(&value).unwrap_or_default()}],"structuredContent":value}),
        ),
        Err(error) => mcp_tool_error(id, error),
    }
}

fn mcp_tool_error(id: serde_json::Value, error: NoteUseCaseError) -> JsonRpcResponse {
    let value = match error {
        NoteUseCaseError::Validation(diagnostics) => validation_problem_json(diagnostics),
        NoteUseCaseError::NotFound => {
            serde_json::json!({"code":"not_found","message":"note was not found"})
        }
        NoteUseCaseError::Conflict => {
            serde_json::json!({"code":"conflict","message":"note revision conflicts"})
        }
        NoteUseCaseError::RenderFailed => serde_json::json!({
            "code":"render_failed",
            "message":"note cannot be rendered safely"
        }),
        NoteUseCaseError::Unavailable => serde_json::json!({
            "code":"unavailable",
            "message":"note service is unavailable"
        }),
    };
    let text = serde_json::to_string(&value).unwrap_or_else(|_| {
        r#"{"code":"unavailable","message":"note service is unavailable"}"#.into()
    });
    JsonRpcResponse::success(
        id,
        serde_json::json!({
            "content":[{"type":"text","text":text}],
            "structuredContent":value,
            "isError":true
        }),
    )
}

fn note_profile_json(profile: NoteProfile) -> serde_json::Value {
    serde_json::json!({
        "profile_version": profile.profile_version,
        "adocweave_package_version": profile.adocweave_package_version,
        "limits": {
            "applies_after_normalization": true,
            "max_title_characters": profile.limits.max_title_characters,
            "max_source_bytes": profile.limits.max_source_bytes,
            "max_tags": profile.limits.max_tags,
            "max_tag_characters": profile.limits.max_tag_characters,
        },
        "normalization": {
            "title": profile.normalization.title,
            "tags": profile.normalization.tags,
        },
        "syntax": {
            "common_blocks": profile.syntax.common_blocks,
            "common_inlines": profile.syntax.common_inlines,
            "source_language_optional": profile.syntax.source_language_optional,
            "allowed_math_languages": profile.syntax.allowed_math_languages,
            "title_forbidden": profile.syntax.title_forbidden,
            "tag_forbidden": profile.syntax.tag_forbidden,
        },
        "allowed_source_languages": profile.allowed_source_languages,
        "forbidden_rules": profile.forbidden_rules.into_iter().map(|rule| serde_json::json!({
            "code": rule.code.as_str(),
            "description": rule.description,
        })).collect::<Vec<_>>(),
        "examples": profile.examples.into_iter().map(|example| serde_json::json!({
            "kind": example.kind,
            "description": example.description,
            "body": example.body,
        })).collect::<Vec<_>>(),
    })
}

fn note_revision_json(note: Note) -> serde_json::Value {
    serde_json::json!({
        "note_id": note.note_id().to_string(),
        "revision": note.revision().get(),
    })
}
