//! MCP requestのBearer token、scope、browser origin検証。

use axum::{
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use marginalis_application::McpAccessTokenAuthenticationError;
use marginalis_contract::ProblemCode;
use marginalis_domain::McpAuthenticatedActor;

use super::super::{
    error::{HandlerResult, problem},
    state::{ApiState, McpEndpoint},
};

pub(super) enum BearerToken<'a> {
    Missing,
    Malformed,
    Value(&'a str),
}

pub(super) fn bearer_token(headers: &HeaderMap) -> BearerToken<'_> {
    let Some(value) = headers.get(header::AUTHORIZATION) else {
        return BearerToken::Missing;
    };
    let Ok(value) = value.to_str() else {
        return BearerToken::Malformed;
    };
    let mut parts = value.split_ascii_whitespace();
    let Some(scheme) = parts.next() else {
        return BearerToken::Malformed;
    };
    let Some(token) = parts.next() else {
        return BearerToken::Malformed;
    };
    if scheme.eq_ignore_ascii_case("bearer") && !token.is_empty() && parts.next().is_none() {
        BearerToken::Value(token)
    } else {
        BearerToken::Malformed
    }
}

pub(super) fn authentication_challenge(
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

pub(super) async fn authenticate(
    endpoint: &McpEndpoint,
    token: &str,
    accepted_scopes: &[&str],
) -> HandlerResult<Result<McpAuthenticatedActor, Response>> {
    let challenged_scope = accepted_scopes.first().copied().unwrap_or("notes:read");
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
            return Ok(Err(authentication_challenge(
                endpoint,
                StatusCode::UNAUTHORIZED,
                Some("invalid_token"),
                challenged_scope,
            )));
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
        return Ok(Err(authentication_challenge(
            endpoint,
            StatusCode::UNAUTHORIZED,
            Some("invalid_token"),
            challenged_scope,
        )));
    };
    if !accepted_scopes.is_empty()
        && !accepted_scopes
            .iter()
            .any(|required| authenticated.scopes.iter().any(|scope| scope == required))
    {
        tracing::warn!(
            event = "mcp.authorization.failed",
            reason = "insufficient-scope",
            required_scope = challenged_scope,
            "MCP access token has insufficient scope"
        );
        return Ok(Err(authentication_challenge(
            endpoint,
            StatusCode::FORBIDDEN,
            Some("insufficient_scope"),
            challenged_scope,
        )));
    }
    Ok(Ok(authenticated))
}
