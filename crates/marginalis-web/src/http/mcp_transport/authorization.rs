//! MCP browser requestのOrigin検証。

use axum::http::{HeaderMap, StatusCode, header};
use marginalis_contract::ProblemCode;

use super::super::{
    error::{HandlerResult, problem},
    state::{ApiState, McpEndpoint},
};

pub(super) fn validate_mcp_origin(
    state: &ApiState,
    endpoint: &McpEndpoint,
    headers: &HeaderMap,
) -> HandlerResult<()> {
    let Some(value) = headers.get(header::ORIGIN) else {
        return Ok(());
    };
    let origin = value.to_str().map_err(|_| {
        tracing::warn!(
            event = "mcp.request.rejected",
            reason = "invalid-origin",
            "rejected MCP browser request with an invalid origin header"
        );
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
        event = "mcp.request.rejected",
        reason = "origin-not-allowed",
        "rejected MCP browser request from an untrusted origin"
    );
    Err(problem(
        StatusCode::FORBIDDEN,
        ProblemCode::OriginNotAllowed,
        "MCP browser request origin is not allowed",
    ))
}
