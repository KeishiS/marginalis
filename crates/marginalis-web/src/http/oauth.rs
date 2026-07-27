//! Marginalisが提供するMCP OAuth authorization server境界。

use std::{collections::HashMap, time::Instant};

use axum::{
    Form, Json,
    body::Bytes,
    extract::{Path, RawQuery, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{Html, IntoResponse, Redirect, Response},
};
use marginalis_application::{
    McpAuthorizationRequest, McpOAuthUseCaseError, McpValidatedAuthorizationRequest,
};
use serde::{Deserialize, Serialize};

use super::{
    auth::{
        CSRF_COOKIE, authenticated_actor, authenticated_form_actor, authenticated_mutation_actor,
        cookie_value, external_path,
    },
    error::{HandlerResult, problem},
    html::escape_html,
    mcp_endpoint,
    state::ApiState,
};

const AUTHORIZATION_PARAMETERS: &[&str] = &[
    "response_type",
    "client_id",
    "redirect_uri",
    "resource",
    "scope",
    "code_challenge",
    "code_challenge_method",
    "state",
];
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
const MAX_LOGIN_RESUME_PATH_BYTES: usize = 2_800;

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

pub(super) async fn mcp_server_metadata(
    State(state): State<ApiState>,
) -> HandlerResult<Json<serde_json::Value>> {
    let endpoint = mcp_endpoint(&state)?;
    Ok(Json(serde_json::json!({
        "issuer": endpoint.authorization_server_uri,
        "authorization_endpoint": endpoint.authorization_endpoint_uri,
        "token_endpoint": endpoint.token_endpoint_uri,
        "registration_endpoint": format!(
            "{}/oauth/register",
            endpoint.authorization_server_uri.trim_end_matches('/')
        ),
        "scopes_supported": ["notes:read", "notes:write", "notes:delete"],
        "response_types_supported": ["code"],
        "grant_types_supported": ["authorization_code", "refresh_token"],
        "code_challenge_methods_supported": ["S256"],
        "token_endpoint_auth_methods_supported": ["none"]
    })))
}

#[derive(Default)]
struct OAuthParameters {
    values: HashMap<String, String>,
    repeated: bool,
}

impl OAuthParameters {
    fn append(&mut self, encoded: &str, known: &[&str]) {
        for (name, value) in url::form_urlencoded::parse(encoded.as_bytes()) {
            if !known.contains(&name.as_ref()) || value.is_empty() {
                continue;
            }
            if self
                .values
                .insert(name.into_owned(), value.into_owned())
                .is_some()
            {
                self.repeated = true;
            }
        }
    }

    fn get(&self, name: &str) -> Option<&str> {
        self.values.get(name).map(String::as_str)
    }

    fn take(&mut self, name: &str) -> Option<String> {
        self.values.remove(name)
    }
}

#[derive(Clone)]
struct McpAuthorizeInput {
    response_type: Option<String>,
    client_id: Option<String>,
    redirect_uri: Option<String>,
    resource: Option<String>,
    scope: Option<String>,
    code_challenge: Option<String>,
    code_challenge_method: Option<String>,
    state: Option<String>,
}

impl McpAuthorizeInput {
    fn from_parameters(parameters: &OAuthParameters) -> Self {
        Self {
            response_type: parameters.get("response_type").map(str::to_owned),
            client_id: parameters.get("client_id").map(str::to_owned),
            redirect_uri: parameters.get("redirect_uri").map(str::to_owned),
            resource: parameters.get("resource").map(str::to_owned),
            scope: parameters.get("scope").map(str::to_owned),
            code_challenge: parameters.get("code_challenge").map(str::to_owned),
            code_challenge_method: parameters.get("code_challenge_method").map(str::to_owned),
            state: parameters.get("state").map(str::to_owned),
        }
    }

    fn scopes(&self) -> Vec<String> {
        self.scope
            .as_deref()
            .unwrap_or_default()
            .split_ascii_whitespace()
            .map(str::to_owned)
            .collect()
    }
}

#[derive(Deserialize)]
pub(super) struct McpAuthorizeForm {
    client_id: String,
    redirect_uri: String,
    resource: String,
    scope: String,
    code_challenge: String,
    state: Option<String>,
    csrf_token: String,
    decision: String,
}

#[derive(Serialize)]
struct McpTokenResponse {
    access_token: String,
    refresh_token: String,
    token_type: &'static str,
    expires_in: u64,
    scope: String,
}

#[derive(Serialize)]
struct OAuthErrorResponse {
    error: &'static str,
    error_description: &'static str,
}

fn oauth_error_response(
    status: StatusCode,
    error: &'static str,
    description: &'static str,
) -> Response {
    (
        status,
        [
            (header::CACHE_CONTROL, "no-store"),
            (header::PRAGMA, "no-cache"),
        ],
        Json(OAuthErrorResponse {
            error,
            error_description: description,
        }),
    )
        .into_response()
}

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

pub(super) async fn mcp_register_client(
    State(state): State<ApiState>,
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

pub(super) async fn mcp_authorize(
    State(state): State<ApiState>,
    headers: HeaderMap,
    RawQuery(raw_query): RawQuery,
) -> Result<Response, Response> {
    let mut parameters = OAuthParameters::default();
    parameters.append(
        raw_query.as_deref().unwrap_or_default(),
        AUTHORIZATION_PARAMETERS,
    );
    mcp_authorize_request(&state, &headers, parameters).await
}

/// OAuth clientからの初回POSTはqueryとform bodyの両方を受け付けるが、状態を変更しない。
pub(super) async fn mcp_authorize_post(
    State(state): State<ApiState>,
    headers: HeaderMap,
    RawQuery(raw_query): RawQuery,
    body: Bytes,
) -> Result<Response, Response> {
    let mut parameters = OAuthParameters::default();
    parameters.append(
        raw_query.as_deref().unwrap_or_default(),
        AUTHORIZATION_PARAMETERS,
    );
    if !body.is_empty() {
        if !content_type_is(&headers, "application/x-www-form-urlencoded") {
            return Err(oauth_error_response(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                "authorization request body must be form encoded",
            ));
        }
        let encoded = std::str::from_utf8(&body).map_err(|_| {
            oauth_error_response(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                "authorization request encoding is invalid",
            )
        })?;
        parameters.append(encoded, AUTHORIZATION_PARAMETERS);
    }
    mcp_authorize_request(&state, &headers, parameters).await
}

async fn mcp_authorize_request(
    state: &ApiState,
    headers: &HeaderMap,
    parameters: OAuthParameters,
) -> Result<Response, Response> {
    let endpoint = mcp_endpoint(state).map_err(|error| error.into_response())?;
    let input = McpAuthorizeInput::from_parameters(&parameters);
    let client_id = input.client_id.clone().ok_or_else(|| {
        oauth_error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "client_id is required",
        )
    })?;
    let resolved = endpoint
        .oauth
        .resolve_authorization_client(client_id.clone(), input.redirect_uri.clone())
        .await
        .map_err(unsafe_authorization_error)?;
    if parameters.repeated {
        return Ok(oauth_redirect_error(
            &resolved.redirect_uri,
            input.state.as_deref(),
            "invalid_request",
        ));
    }
    if input.response_type.as_deref() != Some("code") {
        return Ok(oauth_redirect_error(
            &resolved.redirect_uri,
            input.state.as_deref(),
            "unsupported_response_type",
        ));
    }
    if input.code_challenge_method.as_deref() != Some("S256") {
        return Ok(oauth_redirect_error(
            &resolved.redirect_uri,
            input.state.as_deref(),
            "invalid_request",
        ));
    }
    let request = McpAuthorizationRequest {
        client_id,
        redirect_uri: Some(resolved.redirect_uri.clone()),
        resource_uri: input.resource.clone().unwrap_or_default(),
        scopes: input.scopes(),
        code_challenge: input.code_challenge.clone().unwrap_or_default(),
    };
    let validated = endpoint
        .oauth
        .validate_authorization_request(request)
        .await
        .map_err(|error| {
            safe_authorization_error(&resolved.redirect_uri, input.state.as_deref(), error)
        })?;
    match authenticated_actor(headers, state).await {
        Ok(_) => {}
        Err((StatusCode::UNAUTHORIZED, _)) => {
            return Ok(
                login_redirect(state, &input, &validated).unwrap_or_else(|| {
                    oauth_redirect_error(
                        &validated.redirect_uri,
                        input.state.as_deref(),
                        "invalid_request",
                    )
                }),
            );
        }
        Err(error) => return Err(error.into_response()),
    }
    let csrf = cookie_value(headers, CSRF_COOKIE).ok_or_else(|| {
        problem(
            StatusCode::FORBIDDEN,
            "csrf_required",
            "CSRF token is required",
        )
        .into_response()
    })?;
    Ok(consent_page(state, &input, &validated, &csrf))
}

fn login_redirect(
    state: &ApiState,
    input: &McpAuthorizeInput,
    request: &McpValidatedAuthorizationRequest,
) -> Option<Response> {
    let mut request_uri = url::Url::parse("https://invalid.example/oauth/authorize")
        .expect("constant authorization URL is valid");
    {
        let mut pairs = request_uri.query_pairs_mut();
        pairs.append_pair("response_type", "code");
        pairs.append_pair("client_id", &request.client.client_id);
        pairs.append_pair("redirect_uri", &request.redirect_uri);
        pairs.append_pair("resource", &request.resource_uri);
        pairs.append_pair("scope", &request.scopes.join(" "));
        pairs.append_pair("code_challenge", &request.code_challenge);
        pairs.append_pair("code_challenge_method", "S256");
        if let Some(state) = &input.state {
            pairs.append_pair("state", state);
        }
    }
    let next = format!(
        "{}?{}",
        external_path(&state.cookie_path, "/oauth/authorize"),
        request_uri.query().expect("query pairs were added")
    );
    if next.len() > MAX_LOGIN_RESUME_PATH_BYTES {
        return None;
    }
    let encoded_next = url::form_urlencoded::byte_serialize(next.as_bytes()).collect::<String>();
    Some(
        Redirect::to(&format!(
            "{}?next={encoded_next}",
            external_path(&state.cookie_path, "/auth/oidc/login")
        ))
        .into_response(),
    )
}

fn consent_page(
    state: &ApiState,
    input: &McpAuthorizeInput,
    request: &McpValidatedAuthorizationRequest,
    csrf: &str,
) -> Response {
    let consent_path = external_path(&state.cookie_path, "/oauth/authorize/consent");
    let state_field = input.state.as_deref().map_or_else(String::new, |value| {
        format!(
            "<input type=\"hidden\" name=\"state\" value=\"{}\">",
            escape_html(value)
        )
    });
    let redirect_host = url::Url::parse(&request.redirect_uri)
        .ok()
        .and_then(|url| url.host_str().map(str::to_owned))
        .unwrap_or_else(|| "unknown".into());
    let loopback_warning = url::Url::parse(&request.redirect_uri)
        .ok()
        .is_some_and(|url| url.scheme() == "http");
    let loopback_warning = if loopback_warning {
        "<p>This client redirects to a local application on this device.</p>"
    } else {
        ""
    };
    Html(format!(
        "<!doctype html><meta charset=\"utf-8\"><title>MCP authorization</title><main><h1>Authorize {}</h1><p>Requested scopes: {}</p><p>Redirect host: {}</p>{}<form method=\"post\" action=\"{}\"><input type=\"hidden\" name=\"client_id\" value=\"{}\"><input type=\"hidden\" name=\"redirect_uri\" value=\"{}\"><input type=\"hidden\" name=\"resource\" value=\"{}\"><input type=\"hidden\" name=\"scope\" value=\"{}\"><input type=\"hidden\" name=\"code_challenge\" value=\"{}\">{}<input type=\"hidden\" name=\"csrf_token\" value=\"{}\"><button name=\"decision\" value=\"approve\">Allow</button><button name=\"decision\" value=\"deny\">Deny</button></form></main>",
        escape_html(&request.client.display_name),
        escape_html(&request.scopes.join(" ")),
        escape_html(&redirect_host),
        loopback_warning,
        escape_html(&consent_path),
        escape_html(&request.client.client_id),
        escape_html(&request.redirect_uri),
        escape_html(&request.resource_uri),
        escape_html(&request.scopes.join(" ")),
        escape_html(&request.code_challenge),
        state_field,
        escape_html(csrf)
    ))
    .into_response()
}

/// Marginalis自身が表示した承認form専用の状態変更endpoint。
pub(super) async fn mcp_authorize_consent(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Form(form): Form<McpAuthorizeForm>,
) -> Result<Response, Response> {
    let endpoint = mcp_endpoint(&state).map_err(|error| error.into_response())?;
    let actor = authenticated_form_actor(&headers, &state, &form.csrf_token)
        .await
        .map_err(|error| error.into_response())?;
    let state_value = form.state.as_deref().filter(|value| !value.is_empty());
    let request = McpAuthorizationRequest {
        client_id: form.client_id,
        redirect_uri: Some(form.redirect_uri),
        resource_uri: form.resource,
        scopes: form
            .scope
            .split_ascii_whitespace()
            .map(str::to_owned)
            .collect(),
        code_challenge: form.code_challenge,
    };
    let validated = endpoint
        .oauth
        .validate_authorization_request(request)
        .await
        .map_err(unsafe_authorization_error)?;
    match form.decision.as_str() {
        "deny" => Ok(oauth_redirect_error(
            &validated.redirect_uri,
            state_value,
            "access_denied",
        )),
        "approve" => {
            let redirect_uri = validated.redirect_uri.clone();
            let code = endpoint
                .oauth
                .authorize(actor, validated)
                .await
                .map_err(unsafe_authorization_error)?;
            Ok(oauth_redirect(
                &redirect_uri,
                state_value,
                Some(&code),
                None,
            ))
        }
        _ => Err(oauth_error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "authorization decision is invalid",
        )),
    }
}

fn unsafe_authorization_error(error: McpOAuthUseCaseError) -> Response {
    match error {
        McpOAuthUseCaseError::Unavailable => oauth_error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "temporarily_unavailable",
            "OAuth service is unavailable",
        ),
        _ => oauth_error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "authorization request is invalid",
        ),
    }
}

fn safe_authorization_error(
    redirect_uri: &str,
    state: Option<&str>,
    error: McpOAuthUseCaseError,
) -> Response {
    let code = match error {
        McpOAuthUseCaseError::InvalidScope => "invalid_scope",
        McpOAuthUseCaseError::InvalidTarget => "invalid_target",
        McpOAuthUseCaseError::Unavailable => "server_error",
        _ => "invalid_request",
    };
    oauth_redirect_error(redirect_uri, state, code)
}

fn oauth_redirect_error(redirect_uri: &str, state: Option<&str>, error: &'static str) -> Response {
    oauth_redirect(redirect_uri, state, None, Some(error))
}

fn oauth_redirect(
    redirect_uri: &str,
    state: Option<&str>,
    code: Option<&str>,
    error: Option<&str>,
) -> Response {
    let Ok(mut url) = url::Url::parse(redirect_uri) else {
        return oauth_error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "redirect URI is invalid",
        );
    };
    {
        let mut pairs = url.query_pairs_mut();
        if let Some(code) = code {
            pairs.append_pair("code", code);
        }
        if let Some(error) = error {
            pairs.append_pair("error", error);
        }
        if let Some(state) = state {
            pairs.append_pair("state", state);
        }
    }
    Redirect::to(url.as_str()).into_response()
}

pub(super) async fn mcp_token(
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
            endpoint
                .oauth
                .exchange_authorization_code(
                    code,
                    client_id,
                    form.take("redirect_uri"),
                    resource,
                    verifier,
                )
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

fn content_type_is(headers: &HeaderMap, expected: &str) -> bool {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .is_some_and(|value| value.trim().eq_ignore_ascii_case(expected))
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

pub(super) async fn revoke_mcp_authorization(
    State(state): State<ApiState>,
    Path(client_id): Path<String>,
    headers: HeaderMap,
) -> HandlerResult<StatusCode> {
    let actor = authenticated_mutation_actor(&headers, &state).await?;
    let endpoint = mcp_endpoint(&state)?;
    if client_id.trim().is_empty() {
        return Err(problem(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "client ID is invalid",
        ));
    }
    endpoint.oauth.revoke(actor, client_id).await.map_err(|_| {
        problem(
            StatusCode::SERVICE_UNAVAILABLE,
            "unavailable",
            "OAuth service is unavailable",
        )
    })?;
    Ok(StatusCode::NO_CONTENT)
}
