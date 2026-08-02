use std::time::Instant;

use axum::{
    Json,
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use marginalis_application::McpOAuthUseCaseError;
use serde::{Deserialize, Serialize};

use super::{
    super::{mcp_endpoint, state::ApiState},
    common::{content_type_is, log_mcp_oauth_result, oauth_error_response},
};

#[derive(Deserialize)]
struct McpRegistrationRequest {
    client_name: Option<String>,
    redirect_uris: Option<Vec<String>>,
    token_endpoint_auth_method: Option<String>,
    grant_types: Option<Vec<String>>,
    response_types: Option<Vec<String>>,
}

#[derive(Serialize)]
struct McpRegistrationResponse {
    client_id: String,
    client_name: String,
    redirect_uris: Vec<String>,
    token_endpoint_auth_method: &'static str,
    grant_types: [&'static str; 2],
    response_types: [&'static str; 1],
}

pub(crate) async fn mcp_register_client(
    State(state): State<ApiState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, Response> {
    let result = mcp_register_client_inner(state, headers, body).await;
    log_mcp_oauth_result("registration", &result);
    result
}

async fn mcp_register_client_inner(
    state: ApiState,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, Response> {
    if !content_type_is(&headers, "application/json") {
        return Err(oauth_error_response(
            StatusCode::BAD_REQUEST,
            "invalid_client_metadata",
            "registration request must be JSON",
        ));
    }
    let request = serde_json::from_slice::<McpRegistrationRequest>(&body).map_err(|_| {
        oauth_error_response(
            StatusCode::BAD_REQUEST,
            "invalid_client_metadata",
            "client metadata is invalid",
        )
    })?;
    if request
        .token_endpoint_auth_method
        .as_deref()
        .is_some_and(|method| method != "none")
        || request.grant_types.as_ref().is_some_and(|values| {
            values
                .iter()
                .any(|value| !matches!(value.as_str(), "authorization_code" | "refresh_token"))
        })
        || request
            .response_types
            .as_ref()
            .is_some_and(|values| values.iter().any(|value| value != "code"))
    {
        return Err(oauth_error_response(
            StatusCode::BAD_REQUEST,
            "invalid_client_metadata",
            "client metadata is unsupported",
        ));
    }
    let redirect_uris = request.redirect_uris.ok_or_else(|| {
        oauth_error_response(
            StatusCode::BAD_REQUEST,
            "invalid_client_metadata",
            "redirect_uris is required",
        )
    })?;
    let endpoint = mcp_endpoint(&state).map_err(|error| error.into_response())?;
    let client = marginalis_domain::McpOAuthClient {
        client_id: format!("mcp-{}", uuid::Uuid::now_v7()),
        display_name: request
            .client_name
            .filter(|name| !name.trim().is_empty())
            .unwrap_or_else(|| "MCP client".into()),
        redirect_uris,
    };
    let rate_limit_key = client
        .redirect_uris
        .first()
        .and_then(|value| url::Url::parse(value).ok())
        .map_or_else(
            || "invalid-redirect-uri".into(),
            |url| url.origin().ascii_serialization(),
        );
    if !state
        .mcp_registration_limiter
        .allow(&rate_limit_key, Instant::now())
    {
        return Err(oauth_error_response(
            StatusCode::TOO_MANY_REQUESTS,
            "temporarily_unavailable",
            "dynamic client registration rate limit exceeded",
        ));
    }
    endpoint
        .oauth
        .register_client(client.clone())
        .await
        .map_err(|error| match error {
            McpOAuthUseCaseError::InvalidRedirectUri => oauth_error_response(
                StatusCode::BAD_REQUEST,
                "invalid_redirect_uri",
                "redirect URI is invalid",
            ),
            McpOAuthUseCaseError::Capacity => oauth_error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                "temporarily_unavailable",
                "dynamic client registration is at capacity",
            ),
            McpOAuthUseCaseError::Unavailable => oauth_error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                "temporarily_unavailable",
                "OAuth service is unavailable",
            ),
            _ => oauth_error_response(
                StatusCode::BAD_REQUEST,
                "invalid_client_metadata",
                "client metadata is invalid",
            ),
        })?;
    Ok((
        StatusCode::CREATED,
        [
            (header::CACHE_CONTROL, "no-store"),
            (header::PRAGMA, "no-cache"),
        ],
        Json(McpRegistrationResponse {
            client_id: client.client_id,
            client_name: client.display_name,
            redirect_uris: client.redirect_uris,
            token_endpoint_auth_method: "none",
            grant_types: ["authorization_code", "refresh_token"],
            response_types: ["code"],
        }),
    )
        .into_response())
}
