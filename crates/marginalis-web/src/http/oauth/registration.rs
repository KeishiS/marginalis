use std::time::Instant;

use axum::{
    Json,
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use marginalis_application::McpOAuthUseCaseError;
use mcp_authorization_server::{
    DynamicClientRegistrationError, DynamicClientRegistrationRequest,
    DynamicClientRegistrationResponse, validate_dynamic_client_registration,
};

use super::{
    super::{mcp_endpoint, state::ApiState},
    common::{content_type_is, log_mcp_oauth_result, oauth_error_response},
};

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
    let request =
        serde_json::from_slice::<DynamicClientRegistrationRequest>(&body).map_err(|_| {
            oauth_error_response(
                StatusCode::BAD_REQUEST,
                "invalid_client_metadata",
                "client metadata is invalid",
            )
        })?;
    let endpoint = mcp_endpoint(&state).map_err(|error| error.into_response())?;
    let registration = validate_dynamic_client_registration(
        request,
        format!("mcp-{}", uuid::Uuid::now_v7()),
        "MCP client",
    )
    .map_err(|error| match error {
        DynamicClientRegistrationError::MissingRedirectUris => oauth_error_response(
            StatusCode::BAD_REQUEST,
            "invalid_client_metadata",
            "redirect_uris is required",
        ),
        DynamicClientRegistrationError::UnsupportedMetadata => oauth_error_response(
            StatusCode::BAD_REQUEST,
            "invalid_client_metadata",
            "client metadata is unsupported",
        ),
    })?;
    let client = registration.client;
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
        Json(DynamicClientRegistrationResponse::new(
            client,
            registration.application_type,
        )),
    )
        .into_response())
}
