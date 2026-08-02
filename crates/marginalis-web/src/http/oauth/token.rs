use axum::{
    Json,
    body::Bytes,
    extract::State,
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use marginalis_application::McpOAuthUseCaseError;
use serde::Serialize;

use super::{
    super::{mcp_endpoint, state::ApiState},
    common::{OAuthParameters, content_type_is, oauth_error_response},
};

const TOKEN_PARAMETERS: &[&str] = &[
    "grant_type",
    "code",
    "client_id",
    "redirect_uri",
    "resource",
    "code_verifier",
    "refresh_token",
    "scope",
];

#[derive(Serialize)]
struct McpTokenResponse {
    access_token: String,
    refresh_token: String,
    token_type: &'static str,
    expires_in: u64,
    scope: String,
}

pub(crate) async fn mcp_token(
    State(state): State<ApiState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, Response> {
    let result = mcp_token_inner(state, headers, body).await;
    match &result {
        Ok(_) => tracing::info!(
            event = "mcp.oauth.token.completed",
            "MCP OAuth token request succeeded"
        ),
        Err(response) => tracing::warn!(
            event = "mcp.oauth.token.failed",
            status = response.status().as_u16(),
            "MCP OAuth token request failed"
        ),
    }
    result
}

async fn mcp_token_inner(
    state: ApiState,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, Response> {
    if let Some(authorization) = headers.get(header::AUTHORIZATION) {
        let mut response = oauth_error_response(
            StatusCode::UNAUTHORIZED,
            "invalid_client",
            "client authentication is unsupported",
        );
        if let Some(scheme) = authorization
            .to_str()
            .ok()
            .and_then(|value| value.split_ascii_whitespace().next())
            .filter(|scheme| valid_http_auth_scheme(scheme))
        {
            response.headers_mut().insert(
                header::WWW_AUTHENTICATE,
                HeaderValue::from_str(scheme).expect("validated HTTP authentication scheme"),
            );
        }
        return Err(response);
    }
    if !content_type_is(&headers, "application/x-www-form-urlencoded") {
        return Err(oauth_error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "token request must be form encoded",
        ));
    }
    let encoded = std::str::from_utf8(&body).map_err(|_| {
        oauth_error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "token request encoding is invalid",
        )
    })?;
    let mut form = OAuthParameters::default();
    form.append(encoded, TOKEN_PARAMETERS);
    if form.repeated {
        return Err(oauth_error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "token parameters must not be repeated",
        ));
    }
    let grant_type =
        required_token_parameter(&mut form, "grant_type").map_err(|_| missing_token_parameter())?;
    let client_id =
        required_token_parameter(&mut form, "client_id").map_err(|_| missing_token_parameter())?;
    let resource =
        required_token_parameter(&mut form, "resource").map_err(|_| missing_token_parameter())?;
    let endpoint = mcp_endpoint(&state).map_err(|_| {
        oauth_error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "temporarily_unavailable",
            "OAuth service is unavailable",
        )
    })?;
    let pair = match grant_type.as_str() {
        "authorization_code" => {
            let code = required_token_parameter(&mut form, "code")
                .map_err(|_| missing_token_parameter())?;
            let verifier = required_token_parameter(&mut form, "code_verifier")
                .map_err(|_| missing_token_parameter())?;
            let redirect_uri = form.take("redirect_uri");
            endpoint
                .oauth
                .exchange_authorization_code(code, client_id, redirect_uri, resource, verifier)
                .await
                .map_err(token_use_case_error)?
        }
        "refresh_token" => {
            let refresh_token = required_token_parameter(&mut form, "refresh_token")
                .map_err(|_| missing_token_parameter())?;
            let scopes = form
                .take("scope")
                .map(|value| value.split_ascii_whitespace().map(str::to_owned).collect());
            endpoint
                .oauth
                .refresh_access_token(refresh_token, client_id, resource, scopes)
                .await
                .map_err(token_use_case_error)?
        }
        _ => {
            return Err(oauth_error_response(
                StatusCode::BAD_REQUEST,
                "unsupported_grant_type",
                "OAuth grant type is unsupported",
            ));
        }
    };
    Ok((
        [
            (header::CACHE_CONTROL, "no-store"),
            (header::PRAGMA, "no-cache"),
        ],
        Json(McpTokenResponse {
            access_token: pair.access_token,
            refresh_token: pair.refresh_token,
            token_type: "Bearer",
            expires_in: pair.access_expires_in_seconds,
            scope: pair.scope,
        }),
    )
        .into_response())
}

fn required_token_parameter(
    parameters: &mut OAuthParameters,
    name: &'static str,
) -> Result<String, MissingTokenParameter> {
    parameters.take(name).ok_or(MissingTokenParameter)
}

struct MissingTokenParameter;

fn missing_token_parameter() -> Response {
    oauth_error_response(
        StatusCode::BAD_REQUEST,
        "invalid_request",
        "required token parameter is missing",
    )
}

fn token_use_case_error(error: McpOAuthUseCaseError) -> Response {
    match error {
        McpOAuthUseCaseError::InvalidGrant => oauth_error_response(
            StatusCode::BAD_REQUEST,
            "invalid_grant",
            "OAuth grant is invalid",
        ),
        McpOAuthUseCaseError::InvalidScope => oauth_error_response(
            StatusCode::BAD_REQUEST,
            "invalid_scope",
            "requested scope is invalid",
        ),
        McpOAuthUseCaseError::InvalidTarget => oauth_error_response(
            StatusCode::BAD_REQUEST,
            "invalid_target",
            "requested resource is invalid",
        ),
        McpOAuthUseCaseError::InvalidClient => oauth_error_response(
            StatusCode::BAD_REQUEST,
            "invalid_client",
            "OAuth client is invalid",
        ),
        McpOAuthUseCaseError::Unavailable => oauth_error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "temporarily_unavailable",
            "OAuth service is unavailable",
        ),
        _ => oauth_error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "token request is invalid",
        ),
    }
}

fn valid_http_auth_scheme(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(
                    byte,
                    b'!' | b'#'
                        | b'$'
                        | b'%'
                        | b'&'
                        | b'\''
                        | b'*'
                        | b'+'
                        | b'-'
                        | b'.'
                        | b'^'
                        | b'_'
                        | b'`'
                        | b'|'
                        | b'~'
                )
        })
}
