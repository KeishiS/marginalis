//! MCP Streamable HTTPの入口。

mod authorization;
mod protocol;
mod tools;

use axum::{
    Json,
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};

use crate::mcp::{JsonRpcRequest, JsonRpcResponse};

use super::{error::HandlerResult, mcp_endpoint, state::ApiState};
use authorization::{
    BearerToken, authenticate, authentication_challenge, bearer_token, validate_mcp_origin,
};
use protocol::{
    ProtocolEra, ProtocolValidationError, accepts_media_type, content_type_is_json,
    detected_request_id, initialize_result, supported_versions, valid_tools_list_params,
    validate_protocol,
};
use tools::{decode_tool_call, mcp_tool_call};

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
    let token = match bearer_token(&headers) {
        BearerToken::Value(token) => token,
        BearerToken::Missing => {
            tracing::warn!(
                event = "mcp.authentication.failed",
                reason = "missing-token",
                "MCP access token is missing"
            );
            return Ok(authentication_challenge(
                endpoint,
                StatusCode::UNAUTHORIZED,
                None,
                "notes:read",
            ));
        }
        BearerToken::Malformed => {
            tracing::warn!(
                event = "mcp.authentication.failed",
                reason = "token-format",
                "MCP authorization header is malformed"
            );
            return Ok(authentication_challenge(
                endpoint,
                StatusCode::UNAUTHORIZED,
                Some("invalid_token"),
                "notes:read",
            ));
        }
    };
    if !accepts_media_type(&headers, "application/json")
        || !accepts_media_type(&headers, "text/event-stream")
    {
        return Ok(StatusCode::NOT_ACCEPTABLE.into_response());
    }
    if !content_type_is_json(&headers) {
        return Ok(StatusCode::UNSUPPORTED_MEDIA_TYPE.into_response());
    }

    let request_value = match serde_json::from_slice::<serde_json::Value>(&body) {
        Ok(value) => value,
        Err(_) => {
            return Ok(json_rpc_error_response(
                "unknown",
                serde_json::Value::Null,
                -32700,
                "Parse error",
                "parse-error",
            ));
        }
    };
    if request_value.as_object().is_some_and(|object| {
        !object.contains_key("method")
            && (object.contains_key("result") || object.contains_key("error"))
    }) {
        // このserverはclient向けrequestを送らないため、clientからのresponseも受理しません。
        log_mcp_request("unknown", "rejected", Some("client-response"));
        return Ok(StatusCode::BAD_REQUEST.into_response());
    }
    let invalid_request_id = detected_request_id(&request_value);
    let request = match serde_json::from_value::<JsonRpcRequest>(request_value) {
        Ok(request) => request,
        Err(_) => {
            return Ok(json_rpc_error_response(
                "unknown",
                invalid_request_id,
                -32600,
                "Invalid Request",
                "invalid-request",
            ));
        }
    };
    let method = known_mcp_method(&request.method);
    let protocol_era = match validate_protocol(&headers, &request) {
        Ok(protocol_era) => protocol_era,
        Err(ProtocolValidationError::HeaderMismatch(message)) => {
            log_mcp_request(method, "rejected", Some("header-mismatch"));
            return Ok((
                StatusCode::BAD_REQUEST,
                Json(JsonRpcResponse::error(
                    request.id.response_value(),
                    -32020,
                    message,
                )),
            )
                .into_response());
        }
        Err(ProtocolValidationError::UnsupportedVersion(requested)) => {
            log_mcp_request(method, "rejected", Some("unsupported-version"));
            return Ok((
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
                .into_response());
        }
        Err(ProtocolValidationError::InvalidMetadata) => {
            return Ok(json_rpc_error_response(
                method,
                request.id.response_value(),
                -32602,
                "Invalid params",
                "invalid-modern-metadata",
            ));
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
            return Ok(StatusCode::BAD_REQUEST.into_response());
        }
        let error_id = if request.id.is_valid_mcp_id() {
            id
        } else {
            serde_json::Value::Null
        };
        return Ok(json_rpc_error_response(
            method,
            error_id,
            -32600,
            "Invalid Request",
            "invalid-request",
        ));
    }
    let request_requires_id = matches!(
        request.method.as_str(),
        "initialize" | "ping" | "server/discover" | "tools/list" | "tools/call"
    );
    if request_requires_id && request.id.is_notification() {
        log_mcp_request(method, "rejected", Some("missing-request-id"));
        return Ok(StatusCode::BAD_REQUEST.into_response());
    }
    if request.method == "notifications/initialized" && !request.id.is_notification() {
        return Ok(json_rpc_error_response(
            method,
            id,
            -32600,
            "Invalid Request",
            "unexpected-request-id",
        ));
    }
    if protocol_era == ProtocolEra::Modern && request.id.is_notification() {
        log_mcp_request(method, "rejected", Some("modern-notification"));
        return Ok(StatusCode::BAD_REQUEST.into_response());
    }

    let mut response = match (protocol_era, request.method.as_str()) {
        (ProtocolEra::Modern, "server/discover") => JsonRpcResponse::success(
            id,
            serde_json::json!({
                "resultType": "complete",
                "supportedVersions": supported_versions(),
                "capabilities": {"tools": {}},
                "_meta": server_metadata(),
                "instructions": "Use get_note_profile before creating or updating notes.",
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
    };
    if protocol_era == ProtocolEra::Modern {
        add_modern_result_metadata(&mut response);
    }
    if let Some(error) = response.error.as_ref() {
        log_mcp_request(method, "rejected", Some(json_rpc_error_reason(error.code)));
    } else {
        log_mcp_request(method, "success", None);
    }
    if request.id.is_notification() {
        Ok(StatusCode::ACCEPTED.into_response())
    } else {
        if protocol_era == ProtocolEra::Modern
            && response
                .error
                .as_ref()
                .is_some_and(|error| error.code == -32601)
        {
            Ok((StatusCode::NOT_FOUND, Json(response)).into_response())
        } else {
            Ok(Json(response).into_response())
        }
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
