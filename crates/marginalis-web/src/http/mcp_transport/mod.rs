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
use authorization::{authenticate, authentication_challenge, bearer_token, validate_mcp_origin};
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
    if bearer_token(&headers).is_none() {
        return Ok(authentication_challenge(
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
    if !content_type_is_json(&headers) {
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
        // このserverはclient向けrequestを送らないため、clientからのresponseも受理しません。
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
    if request.method != "initialize" && !protocol_version_is_supported(&headers) {
        return Ok(StatusCode::BAD_REQUEST.into_response());
    }

    let decoded_tool_call =
        (request.method == "tools/call").then(|| decode_tool_call(request.params.clone()));
    let accepted_scopes = decoded_tool_call
        .as_ref()
        .and_then(|call| call.as_ref().ok())
        .map_or(&[][..], |call| call.accepted_scopes());
    let challenged_scope = accepted_scopes.first().copied().unwrap_or("notes:read");
    let Some(token) = bearer_token(&headers) else {
        return Ok(authentication_challenge(
            endpoint,
            StatusCode::UNAUTHORIZED,
            None,
            challenged_scope,
        ));
    };
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
            Ok(call) => mcp_tool_call(state.notes.as_ref(), authenticated.actor, id, call).await,
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
