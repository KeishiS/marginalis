use axum::{
    body::Bytes,
    extract::{Path, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use marginalis_contract::ProblemCode;

use super::super::{
    auth::authenticated_mutation_actor,
    error::{HandlerResult, problem},
    mcp_endpoint,
    state::ApiState,
};
use super::common::{OAuthParameters, content_type_is, oauth_error_response};

const REVOCATION_PARAMETERS: &[&str] = &["token", "token_type_hint", "client_id"];

/// RFC 7009に従い、access tokenまたはrefresh tokenが属するtoken familyを失効する。
pub(crate) async fn mcp_revoke_token(
    State(state): State<ApiState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, Response> {
    if headers.contains_key(header::AUTHORIZATION) {
        return Err(oauth_error_response(
            StatusCode::UNAUTHORIZED,
            "invalid_client",
            "client authentication is unsupported",
        ));
    }
    if !content_type_is(&headers, "application/x-www-form-urlencoded") {
        return Err(oauth_error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "revocation request must be form encoded",
        ));
    }
    let encoded = std::str::from_utf8(&body).map_err(|_| {
        oauth_error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "revocation request encoding is invalid",
        )
    })?;
    let mut form = OAuthParameters::default();
    form.append(encoded, REVOCATION_PARAMETERS);
    let token = form.take("token");
    let client_id = form.take("client_id");
    if form.repeated
        || token.as_deref().is_none_or(str::is_empty)
        || client_id.as_deref().is_none_or(str::is_empty)
    {
        return Err(oauth_error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "token and client_id are required",
        ));
    }
    let endpoint = mcp_endpoint(&state).map_err(|_| {
        oauth_error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "temporarily_unavailable",
            "OAuth service is unavailable",
        )
    })?;
    endpoint
        .oauth
        .revoke_token(
            token.expect("validated token"),
            client_id.expect("validated client ID"),
        )
        .await
        .map_err(|_| {
            oauth_error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                "temporarily_unavailable",
                "OAuth service is unavailable",
            )
        })?;
    Ok(([(header::CACHE_CONTROL, "no-store")], StatusCode::OK).into_response())
}

pub(crate) async fn revoke_mcp_authorization(
    State(state): State<ApiState>,
    Path(client_id): Path<String>,
    headers: HeaderMap,
) -> HandlerResult<StatusCode> {
    let result = revoke_mcp_authorization_inner(state, client_id, headers).await;
    match &result {
        Ok(_) => {
            tracing::info!(
                event = "mcp.oauth.operation.completed",
                operation = "revocation",
                "MCP OAuth operation succeeded"
            );
        }
        Err((status, _)) => {
            tracing::warn!(
                event = "mcp.oauth.operation.failed",
                operation = "revocation",
                status = status.as_u16(),
                "MCP OAuth operation failed"
            );
        }
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
    if client_id.trim().is_empty() || client_id.chars().count() > 2_048 {
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
