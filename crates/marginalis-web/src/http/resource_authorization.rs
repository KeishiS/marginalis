//! OAuthで保護したHTTP resourceに共通するBearer tokenとscopeの検査。

use axum::{
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use marginalis_application::McpAuthenticatedActor;
use marginalis_contract::ProblemCode;

use super::{
    error::{HandlerResult, problem},
    state::McpEndpoint,
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

pub(super) async fn authenticate(
    endpoint: &McpEndpoint,
    token: &str,
    scope_requirements: &[&[&str]],
) -> HandlerResult<Result<McpAuthenticatedActor, Response>> {
    let required_scopes = minimum_required_scopes(scope_requirements);
    let challenged_scope = if required_scopes.is_empty() {
        endpoint.resource_policy.default_scopes().join(" ")
    } else {
        required_scopes.join(" ")
    };
    let authenticated = match endpoint
        .oauth
        .authenticate(token.into(), endpoint.resource_policy.uri().to_string())
        .await
    {
        Ok(authenticated) => authenticated,
        Err(_) => {
            tracing::error!(
                event = "oauth.resource.authentication.unavailable",
                reason = "repository-unavailable",
                "OAuth access token authentication is unavailable"
            );
            return Err(problem(
                StatusCode::SERVICE_UNAVAILABLE,
                ProblemCode::Unavailable,
                "OAuth authentication is unavailable",
            ));
        }
    };
    let Some(authenticated) = authenticated else {
        tracing::warn!(
            event = "oauth.resource.authentication.failed",
            reason = "invalid-token",
            "OAuth access token was rejected"
        );
        return Ok(Err(authentication_challenge(
            endpoint,
            StatusCode::UNAUTHORIZED,
            Some("invalid_token"),
            &challenged_scope,
        )));
    };
    if let Some(challenged_scope) = incremental_scope_challenge(
        endpoint.resource_policy.supported_scopes(),
        &authenticated.scopes,
        scope_requirements,
    ) {
        tracing::warn!(
            event = "oauth.resource.authorization.failed",
            reason = "insufficient-scope",
            required_scope = challenged_scope,
            "OAuth access token has insufficient scope"
        );
        return Ok(Err(authentication_challenge(
            endpoint,
            StatusCode::FORBIDDEN,
            Some("insufficient_scope"),
            &challenged_scope,
        )));
    }
    Ok(Ok(authenticated))
}

fn minimum_required_scopes<'a>(scope_requirements: &'a [&'a [&'a str]]) -> Vec<&'a str> {
    scope_requirements
        .iter()
        .filter_map(|alternatives| alternatives.first().copied())
        .collect()
}

fn incremental_scope_challenge(
    supported_scopes: &[String],
    granted_scopes: &[String],
    scope_requirements: &[&[&str]],
) -> Option<String> {
    let missing_scopes = scope_requirements
        .iter()
        .filter(|alternatives| {
            !alternatives
                .iter()
                .any(|required| granted_scopes.iter().any(|scope| scope == required))
        })
        .filter_map(|alternatives| alternatives.first().copied())
        .collect::<Vec<_>>();
    (!missing_scopes.is_empty()).then(|| {
        supported_scopes
            .iter()
            .filter(|scope| {
                granted_scopes.contains(scope)
                    || missing_scopes.iter().any(|missing| scope == missing)
            })
            .cloned()
            .collect::<Vec<_>>()
            .join(" ")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_challenge_contains_every_missing_requirement_and_existing_scope() {
        let supported = ["notes:read", "notes:write", "notes:delete", "notes:sync"]
            .map(str::to_owned)
            .to_vec();
        let granted = vec!["notes:read".to_owned()];

        assert_eq!(
            incremental_scope_challenge(
                &supported,
                &granted,
                &[&["notes:write"], &["notes:delete"]],
            ),
            Some("notes:read notes:write notes:delete".into())
        );
        assert_eq!(
            incremental_scope_challenge(&supported, &granted, &[&["notes:sync"]]),
            Some("notes:read notes:sync".into())
        );
        assert_eq!(
            incremental_scope_challenge(&supported, &granted, &[&["notes:read", "notes:write"]],),
            None
        );
    }
}
