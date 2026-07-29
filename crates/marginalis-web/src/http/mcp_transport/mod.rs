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
    accepts_media_type, content_type_is_json, detected_request_id, initialize_result,
    protocol_version_is_supported, valid_tools_list_params,
};
use tools::{decode_tool_call, mcp_tool_call};

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
    if request.method != "initialize" && !protocol_version_is_supported(&headers) {
        log_mcp_request(method, "rejected", Some("unsupported-version"));
        return Ok(StatusCode::BAD_REQUEST.into_response());
    }

    let decoded_tool_call =
        (request.method == "tools/call").then(|| decode_tool_call(request.params.clone()));
    let accepted_scopes = decoded_tool_call
        .as_ref()
        .and_then(|call| call.as_ref().ok())
        .map_or(&[][..], |call| call.accepted_scopes());
    let authenticated = match authenticate(endpoint, token, accepted_scopes).await? {
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
        "initialize" | "ping" | "tools/list" | "tools/call"
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
            Ok(call) => mcp_tool_call(state.notes.as_ref(), authenticated.actor, id, call).await,
            Err(()) => JsonRpcResponse::error(id, -32602, "Invalid params"),
        },
        _ => JsonRpcResponse::error(id, -32601, "Method not found"),
    };
    if let Some(error) = response.error.as_ref() {
        log_mcp_request(method, "rejected", Some(json_rpc_error_reason(error.code)));
    } else {
        log_mcp_request(method, "success", None);
    }
    if request.id.is_notification() {
        Ok(StatusCode::ACCEPTED.into_response())
    } else {
        Ok(Json(response).into_response())
    }
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
