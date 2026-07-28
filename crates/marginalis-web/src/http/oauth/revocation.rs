use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
};
use marginalis_contract::ProblemCode;

use super::super::{
    auth::authenticated_mutation_actor,
    error::{HandlerResult, problem},
    mcp_endpoint,
    state::ApiState,
};

pub(crate) async fn revoke_mcp_authorization(
    State(state): State<ApiState>,
    Path(client_id): Path<String>,
    headers: HeaderMap,
) -> HandlerResult<StatusCode> {
    let result = revoke_mcp_authorization_inner(state, client_id, headers).await;
    match &result {
        Ok(_) => tracing::info!(
            event = "mcp.oauth.operation.completed",
            operation = "revocation",
            "MCP OAuth operation succeeded"
        ),
        Err((status, _)) => tracing::warn!(
            event = "mcp.oauth.operation.failed",
            operation = "revocation",
            status = status.as_u16(),
            "MCP OAuth operation failed"
        ),
    }
    result
}

async fn revoke_mcp_authorization_inner(
    state: ApiState,
    client_id: String,
    headers: HeaderMap,
) -> HandlerResult<StatusCode> {
    let actor = authenticated_mutation_actor(&headers, &state).await?;
    let endpoint = mcp_endpoint(&state)?;
    if client_id.trim().is_empty() {
        return Err(problem(
            StatusCode::BAD_REQUEST,
            ProblemCode::InvalidRequest,
            "client ID is invalid",
        ));
    }
    endpoint.oauth.revoke(actor, client_id).await.map_err(|_| {
        problem(
            StatusCode::SERVICE_UNAVAILABLE,
            ProblemCode::Unavailable,
            "OAuth service is unavailable",
        )
    })?;
    Ok(StatusCode::NO_CONTENT)
}
