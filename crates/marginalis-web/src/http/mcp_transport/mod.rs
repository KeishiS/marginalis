//! MCP Streamable HTTPの入口。

mod authorization;
mod jsonrpc;
mod protocol;
mod tools;

use axum::{
    Json,
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};

use marginalis_application::McpAuthenticatedActor;

use super::resource_authorization::{
    BearerToken, authenticate, authentication_challenge, bearer_token,
};
use super::{
    error::HandlerResult,
    mcp_endpoint,
    state::{ApiState, McpEndpoint},
};
use authorization::validate_mcp_origin;
use jsonrpc::{JsonRpcRequest, JsonRpcResponse};
use protocol::{
    ProtocolEra, ProtocolValidationError, accepts_media_type, content_type_is_json,
    detected_request_id, initialize_result, supported_versions, valid_tools_list_params,
    validate_protocol,
};
use tools::{McpToolCall, decode_tool_call, mcp_tool_call};

pub(super) async fn mcp_unsupported_method(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> HandlerResult<Response> {
    let endpoint = mcp_endpoint(&state)?;
    validate_mcp_origin(&state, endpoint, &headers)?;
    Ok(StatusCode::METHOD_NOT_ALLOWED.into_response())
}

/// Streamable HTTPのPOST入口。検査と処理は責務ごとの関数へ分け、順序だけをここで持つ。
pub(super) async fn mcp_post(
    State(state): State<ApiState>,
    headers: HeaderMap,
    body: Bytes,
) -> HandlerResult<Response> {
    let endpoint = mcp_endpoint(&state)?;
    validate_mcp_origin(&state, endpoint, &headers)?;
    let token = match require_bearer_token(endpoint, &headers) {
        Ok(token) => token,
        Err(response) => return Ok(*response),
    };
    if let Some(response) = refuse_unsupported_media_types(&headers) {
        return Ok(response);
    }
    let request = match parse_json_rpc_request(&body) {
        Ok(request) => request,
        Err(response) => return Ok(*response),
    };
    let method = known_mcp_method(&request.method);
    let protocol_era = match select_protocol_era(&headers, &request, method) {
        Ok(protocol_era) => protocol_era,
        Err(response) => return Ok(*response),
    };

    let decoded_tool_call =
        (request.method == "tools/call").then(|| decode_tool_call(request.params.clone()));
    let scope_requirements = decoded_tool_call
        .as_ref()
        .and_then(|call| call.as_ref().ok())
        .map_or(&[][..], |call| call.scope_requirements());
    let authenticated = match authenticate(endpoint, token, scope_requirements).await? {
        Ok(authenticated) => authenticated,
        Err(response) => return Ok(response),
    };

    let id = request.id.response_value();
    if let Err(response) = validate_request_shape(&request, method, protocol_era, &id) {
        return Ok(*response);
    }
    let is_notification = request.id.is_notification();
    let response = dispatch_mcp_method(
        &state,
        authenticated,
        protocol_era,
        request,
        id,
        decoded_tool_call,
    )
    .await;
    Ok(finalize_mcp_response(
        protocol_era,
        method,
        is_notification,
        response,
    ))
}

/// Bearer tokenを取り出す。欠落や形式不正はWWW-Authenticateを伴う401で応答する。
///
/// 以降の検査関数も同様に、拒否応答は`Response`が大きいためBoxで返す。
fn require_bearer_token<'a>(
    endpoint: &McpEndpoint,
    headers: &'a HeaderMap,
) -> Result<&'a str, Box<Response>> {
    match bearer_token(headers) {
        BearerToken::Value(token) => Ok(token),
        BearerToken::Missing => {
            tracing::warn!(
                event = "mcp.authentication.failed",
                reason = "missing-token",
                "MCP access token is missing"
            );
            Err(Box::new(authentication_challenge(
                endpoint,
                StatusCode::UNAUTHORIZED,
                None,
                "notes:read",
            )))
        }
        BearerToken::Malformed => {
            tracing::warn!(
                event = "mcp.authentication.failed",
                reason = "token-format",
                "MCP authorization header is malformed"
            );
            Err(Box::new(authentication_challenge(
                endpoint,
                StatusCode::UNAUTHORIZED,
                Some("invalid_token"),
                "notes:read",
            )))
        }
    }
}

/// AcceptとContent-Typeを検査し、受理できない場合の応答を返す。
fn refuse_unsupported_media_types(headers: &HeaderMap) -> Option<Response> {
    if !accepts_media_type(headers, "application/json")
        || !accepts_media_type(headers, "text/event-stream")
    {
        return Some(StatusCode::NOT_ACCEPTABLE.into_response());
    }
    if !content_type_is_json(headers) {
        return Some(StatusCode::UNSUPPORTED_MEDIA_TYPE.into_response());
    }
    None
}

/// bodyをJSON-RPC requestとして読み取る。response形の入力はここで拒否する。
fn parse_json_rpc_request(body: &[u8]) -> Result<JsonRpcRequest, Box<Response>> {
    let request_value = match serde_json::from_slice::<serde_json::Value>(body) {
        Ok(value) => value,
        Err(_) => {
            return Err(Box::new(json_rpc_error_response(
                "unknown",
                serde_json::Value::Null,
                -32700,
                "Parse error",
                "parse-error",
            )));
        }
    };
    if request_value.as_object().is_some_and(|object| {
        !object.contains_key("method")
            && (object.contains_key("result") || object.contains_key("error"))
    }) {
        // このserverはclient向けrequestを送らないため、clientからのresponseも受理しません。
        log_mcp_request("unknown", "rejected", Some("client-response"));
        return Err(Box::new(StatusCode::BAD_REQUEST.into_response()));
    }
    let invalid_request_id = detected_request_id(&request_value);
    serde_json::from_value::<JsonRpcRequest>(request_value).map_err(|_| {
        Box::new(json_rpc_error_response(
            "unknown",
            invalid_request_id,
            -32600,
            "Invalid Request",
            "invalid-request",
        ))
    })
}

/// protocol headerと`_meta`からLegacy・Modernを判定し、選択結果を記録する。
fn select_protocol_era(
    headers: &HeaderMap,
    request: &JsonRpcRequest,
    method: &'static str,
) -> Result<ProtocolEra, Box<Response>> {
    let protocol_era = match validate_protocol(headers, request) {
        Ok(protocol_era) => protocol_era,
        Err(ProtocolValidationError::HeaderMismatch(message)) => {
            log_mcp_request(method, "rejected", Some("header-mismatch"));
            return Err(Box::new(
                (
                    StatusCode::BAD_REQUEST,
                    Json(JsonRpcResponse::error(
                        request.id.response_value(),
                        -32020,
                        message,
                    )),
                )
                    .into_response(),
            ));
        }
        Err(ProtocolValidationError::UnsupportedVersion(requested)) => {
            log_mcp_request(method, "rejected", Some("unsupported-version"));
            return Err(Box::new(
                (
                    StatusCode::BAD_REQUEST,
                    Json(JsonRpcResponse::error_with_data(
                        request.id.response_value(),
                        -32022,
                        "Unsupported protocol version",
                        serde_json::json!({
                            "supported": supported_versions(),
                            "requested": requested
                        }),
                    )),
                )
                    .into_response(),
            ));
        }
        Err(ProtocolValidationError::InvalidMetadata) => {
            return Err(Box::new(json_rpc_error_response(
                method,
                request.id.response_value(),
                -32602,
                "Invalid params",
                "invalid-modern-metadata",
            )));
        }
    };
    let protocol_version = match protocol_era {
        ProtocolEra::Modern => "2026-07-28",
        ProtocolEra::Legacy => headers
            .get("mcp-protocol-version")
            .and_then(|value| value.to_str().ok())
            .unwrap_or("2025-03-26"),
    };
    tracing::info!(
        event = "mcp.protocol.selected",
        protocol_era = match protocol_era {
            ProtocolEra::Modern => "modern",
            ProtocolEra::Legacy => "legacy",
        },
        protocol_version,
        method,
        "MCP protocol request was classified"
    );
    Ok(protocol_era)
}

/// JSON-RPC 2.0とMCPが要求するrequestの形を検査する。
fn validate_request_shape(
    request: &JsonRpcRequest,
    method: &'static str,
    protocol_era: ProtocolEra,
    id: &serde_json::Value,
) -> Result<(), Box<Response>> {
    let params_are_valid = request
        .params
        .as_ref()
        .is_none_or(serde_json::Value::is_object);
    if request.jsonrpc.as_deref() != Some("2.0")
        || !request.id.is_valid_mcp_id()
        || !params_are_valid
    {
        if request.id.is_notification() && request.jsonrpc.as_deref() == Some("2.0") {
            log_mcp_request(method, "rejected", Some("invalid-notification"));
            return Err(Box::new(StatusCode::BAD_REQUEST.into_response()));
        }
        let error_id = if request.id.is_valid_mcp_id() {
            id.clone()
        } else {
            serde_json::Value::Null
        };
        return Err(Box::new(json_rpc_error_response(
            method,
            error_id,
            -32600,
            "Invalid Request",
            "invalid-request",
        )));
    }
    let request_requires_id = matches!(
        request.method.as_str(),
        "initialize" | "ping" | "server/discover" | "tools/list" | "tools/call"
    );
    if request_requires_id && request.id.is_notification() {
        log_mcp_request(method, "rejected", Some("missing-request-id"));
        return Err(Box::new(StatusCode::BAD_REQUEST.into_response()));
    }
    if request.method == "notifications/initialized" && !request.id.is_notification() {
        return Err(Box::new(json_rpc_error_response(
            method,
            id.clone(),
            -32600,
            "Invalid Request",
            "unexpected-request-id",
        )));
    }
    if protocol_era == ProtocolEra::Modern && request.id.is_notification() {
        log_mcp_request(method, "rejected", Some("modern-notification"));
        return Err(Box::new(StatusCode::BAD_REQUEST.into_response()));
    }
    Ok(())
}

/// 検査済みrequestを、protocol世代とmethodの組でserver処理へ振り分ける。
async fn dispatch_mcp_method(
    state: &ApiState,
    authenticated: McpAuthenticatedActor,
    protocol_era: ProtocolEra,
    request: JsonRpcRequest,
    id: serde_json::Value,
    decoded_tool_call: Option<Result<McpToolCall, ()>>,
) -> JsonRpcResponse {
    match (protocol_era, request.method.as_str()) {
        (ProtocolEra::Modern, "server/discover") => JsonRpcResponse::success(
            id,
            serde_json::json!({
                "resultType": "complete",
                "supportedVersions": supported_versions(),
                "capabilities": {"tools": {}},
                "_meta": server_metadata(),
                "instructions": marginalis_contract::MCP_SERVER_INSTRUCTIONS,
                "ttlMs": 3600000,
                "cacheScope": "private"
            }),
        ),
        (ProtocolEra::Modern, "tools/list") if valid_tools_list_params(request.params.as_ref()) => {
            JsonRpcResponse::success(
                id,
                serde_json::json!({
                    "tools": marginalis_contract::mcp_tool_contracts(),
                    "ttlMs": 3600000,
                    "cacheScope": "private"
                }),
            )
        }
        (ProtocolEra::Modern, "tools/list") => JsonRpcResponse::error(id, -32602, "Invalid params"),
        (ProtocolEra::Modern, "tools/call") => {
            match decoded_tool_call.expect("tools/call was decoded") {
                Ok(call) => {
                    mcp_tool_call(
                        state.notes.as_ref(),
                        state.bibliography.as_deref(),
                        authenticated.actor,
                        id,
                        call,
                    )
                    .await
                }
                Err(()) => JsonRpcResponse::error(id, -32602, "Invalid params"),
            }
        }
        (ProtocolEra::Modern, _) => JsonRpcResponse::error(id, -32601, "Method not found"),
        (ProtocolEra::Legacy, "initialize") => match initialize_result(request.params) {
            Ok(result) => JsonRpcResponse::success(id, result),
            Err(()) => JsonRpcResponse::error(id, -32602, "Invalid params"),
        },
        (ProtocolEra::Legacy, "notifications/initialized") => {
            JsonRpcResponse::success(id, serde_json::json!({}))
        }
        (ProtocolEra::Legacy, "ping") => JsonRpcResponse::success(id, serde_json::json!({})),
        (ProtocolEra::Legacy, "tools/list") if valid_tools_list_params(request.params.as_ref()) => {
            JsonRpcResponse::success(
                id,
                serde_json::json!({"tools": marginalis_contract::mcp_tool_contracts()}),
            )
        }
        (ProtocolEra::Legacy, "tools/list") => JsonRpcResponse::error(id, -32602, "Invalid params"),
        (ProtocolEra::Legacy, "tools/call") => {
            match decoded_tool_call.expect("tools/call was decoded") {
                Ok(call) => {
                    mcp_tool_call(
                        state.notes.as_ref(),
                        state.bibliography.as_deref(),
                        authenticated.actor,
                        id,
                        call,
                    )
                    .await
                }
                Err(()) => JsonRpcResponse::error(id, -32602, "Invalid params"),
            }
        }
        (ProtocolEra::Legacy, _) => JsonRpcResponse::error(id, -32601, "Method not found"),
    }
}

/// Modern世代の共通metadataを補い、完了記録とHTTP statusへの写像を行う。
fn finalize_mcp_response(
    protocol_era: ProtocolEra,
    method: &'static str,
    is_notification: bool,
    mut response: JsonRpcResponse,
) -> Response {
    if protocol_era == ProtocolEra::Modern {
        add_modern_result_metadata(&mut response);
    }
    if let Some(error) = response.error.as_ref() {
        log_mcp_request(method, "rejected", Some(json_rpc_error_reason(error.code)));
    } else {
        log_mcp_request(method, "success", None);
    }
    if is_notification {
        StatusCode::ACCEPTED.into_response()
    } else if protocol_era == ProtocolEra::Modern
        && response
            .error
            .as_ref()
            .is_some_and(|error| error.code == -32601)
    {
        (StatusCode::NOT_FOUND, Json(response)).into_response()
    } else {
        Json(response).into_response()
    }
}

fn server_metadata() -> serde_json::Value {
    serde_json::json!({
        "io.modelcontextprotocol/serverInfo": {
            "name": "marginalis",
            "version": env!("CARGO_PKG_VERSION")
        }
    })
}

fn add_modern_result_metadata(response: &mut JsonRpcResponse) {
    let Some(result) = response
        .result
        .as_mut()
        .and_then(serde_json::Value::as_object_mut)
    else {
        return;
    };
    result
        .entry("resultType")
        .or_insert_with(|| serde_json::Value::String("complete".into()));
    result.entry("_meta").or_insert_with(server_metadata);
}

fn json_rpc_error_response(
    method: &'static str,
    id: serde_json::Value,
    code: i32,
    message: &'static str,
    reason: &'static str,
) -> Response {
    log_mcp_request(method, "rejected", Some(reason));
    Json(JsonRpcResponse::error(id, code, message)).into_response()
}

fn known_mcp_method(method: &str) -> &'static str {
    match method {
        "initialize" => "initialize",
        "notifications/initialized" => "notifications/initialized",
        "server/discover" => "server/discover",
        "ping" => "ping",
        "tools/list" => "tools/list",
        "tools/call" => "tools/call",
        _ => "unknown",
    }
}

fn json_rpc_error_reason(code: i32) -> &'static str {
    match code {
        -32700 => "parse-error",
        -32600 => "invalid-request",
        -32601 => "method-not-found",
        -32602 => "invalid-params",
        -32020 => "header-mismatch",
        -32022 => "unsupported-version",
        _ => "protocol-error",
    }
}

fn log_mcp_request(method: &'static str, outcome: &'static str, reason: Option<&'static str>) {
    if let Some(reason) = reason {
        tracing::info!(
            event = "mcp.request.completed",
            method,
            outcome,
            reason,
            "MCP request was rejected"
        );
    } else {
        tracing::info!(
            event = "mcp.request.completed",
            method,
            outcome,
            "MCP request completed"
        );
    }
}
