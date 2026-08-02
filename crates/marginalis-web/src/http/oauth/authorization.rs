//! Marginalisが提供するMCP OAuth authorization server境界。

use axum::{
    Form, Json,
    body::Bytes,
    extract::{RawQuery, State},
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse, Redirect, Response},
};
use marginalis_application::{
    McpAuthorizationRequest, McpOAuthUseCaseError, McpValidatedAuthorizationRequest,
};
use marginalis_contract::ProblemCode;
use serde::Deserialize;

use super::super::{
    auth::{
        CSRF_COOKIE, authenticated_actor, authenticated_form_actor, cookie_value, external_path,
    },
    error::{HandlerResult, problem},
    html::escape_html,
    mcp_endpoint,
    state::ApiState,
};
use super::common::{OAuthParameters, content_type_is, log_mcp_oauth_result, oauth_error_response};

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
const MAX_LOGIN_RESUME_PATH_BYTES: usize = 2_800;

pub(crate) async fn mcp_resource_metadata(
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

pub(crate) async fn mcp_server_metadata(
    State(state): State<ApiState>,
) -> HandlerResult<Json<serde_json::Value>> {
    let endpoint = mcp_endpoint(&state)?;
    Ok(Json(serde_json::json!({
        "issuer": endpoint.authorization_server_uri,
        "authorization_endpoint": endpoint.authorization_endpoint_uri,
        "token_endpoint": endpoint.token_endpoint_uri,
        "revocation_endpoint": endpoint.revocation_endpoint_uri,
        "registration_endpoint": endpoint.registration_endpoint_uri,
        "protected_resources": [endpoint.resource_uri],
        "scopes_supported": ["notes:read", "notes:write", "notes:delete"],
        "response_types_supported": ["code"],
        "grant_types_supported": ["authorization_code", "refresh_token"],
        "code_challenge_methods_supported": ["S256"],
        "token_endpoint_auth_methods_supported": ["none"],
        "revocation_endpoint_auth_methods_supported": ["none"],
        "authorization_response_iss_parameter_supported": true,
        "client_id_metadata_document_supported": true
    })))
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
pub(crate) struct McpAuthorizeForm {
    client_id: String,
    redirect_uri: String,
    resource: String,
    scope: String,
    code_challenge: String,
    state: Option<String>,
    csrf_token: String,
    decision: String,
}

pub(crate) async fn mcp_authorize(
    State(state): State<ApiState>,
    headers: HeaderMap,
    RawQuery(raw_query): RawQuery,
) -> Result<Response, Response> {
    let mut parameters = OAuthParameters::default();
    parameters.append(
        raw_query.as_deref().unwrap_or_default(),
        AUTHORIZATION_PARAMETERS,
    );
    let result = mcp_authorize_request(&state, &headers, parameters).await;
    log_mcp_oauth_result("authorization", &result);
    result
}

/// OAuth clientからの初回POSTはqueryとform bodyの両方を受け付けるが、状態を変更しない。
pub(crate) async fn mcp_authorize_post(
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
    let result = mcp_authorize_request(&state, &headers, parameters).await;
    log_mcp_oauth_result("authorization", &result);
    result
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
            &endpoint.authorization_server_uri,
            &resolved.redirect_uri,
            input.state.as_deref(),
            "invalid_request",
        ));
    }
    if input.response_type.as_deref() != Some("code") {
        return Ok(oauth_redirect_error(
            &endpoint.authorization_server_uri,
            &resolved.redirect_uri,
            input.state.as_deref(),
            "unsupported_response_type",
        ));
    }
    if input.code_challenge_method.as_deref() != Some("S256") {
        return Ok(oauth_redirect_error(
            &endpoint.authorization_server_uri,
            &resolved.redirect_uri,
            input.state.as_deref(),
            "invalid_request",
        ));
    }
    let request = McpAuthorizationRequest {
        client_id,
        redirect_uri: resolved.redirect_uri.clone(),
        resource_uri: input.resource.clone().unwrap_or_default(),
        scopes: input.scopes(),
        code_challenge: input.code_challenge.clone().unwrap_or_default(),
    };
    let error_redirect_uri = resolved.redirect_uri.clone();
    let validated = endpoint
        .oauth
        .validate_resolved_authorization_request(request, resolved)
        .await
        .map_err(|error| {
            safe_authorization_error(
                &endpoint.authorization_server_uri,
                &error_redirect_uri,
                input.state.as_deref(),
                error,
            )
        })?;
    match authenticated_actor(headers, state).await {
        Ok(_) => {}
        Err((StatusCode::UNAUTHORIZED, _)) => {
            return Ok(
                login_redirect(state, &input, &validated).unwrap_or_else(|| {
                    oauth_redirect_error(
                        &endpoint.authorization_server_uri,
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
            ProblemCode::CsrfRequired,
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
    let client_identifier = url::Url::parse(&request.client.client_id)
        .ok()
        .filter(|url| url.scheme() == "https")
        .and_then(|url| url.host_str().map(str::to_owned))
        .unwrap_or_else(|| request.client.client_id.clone());
    let loopback_warning = url::Url::parse(&request.redirect_uri)
        .ok()
        .is_some_and(|url| url.scheme() == "http");
    let loopback_warning = if loopback_warning {
        "<p>This client redirects to a local application on this device.</p>"
    } else {
        ""
    };
    Html(format!(
        "<!doctype html><meta charset=\"utf-8\"><title>MCP authorization</title><main><h1>Authorize {}</h1><p>Client identifier: {}</p><p>Requested scopes: {}</p><p>Redirect host: {}</p>{}<form method=\"post\" action=\"{}\"><input type=\"hidden\" name=\"client_id\" value=\"{}\"><input type=\"hidden\" name=\"redirect_uri\" value=\"{}\"><input type=\"hidden\" name=\"resource\" value=\"{}\"><input type=\"hidden\" name=\"scope\" value=\"{}\"><input type=\"hidden\" name=\"code_challenge\" value=\"{}\">{}<input type=\"hidden\" name=\"csrf_token\" value=\"{}\"><button name=\"decision\" value=\"approve\">Allow</button><button name=\"decision\" value=\"deny\">Deny</button></form></main>",
        escape_html(&request.client.display_name),
        escape_html(&client_identifier),
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
pub(crate) async fn mcp_authorize_consent(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Form(form): Form<McpAuthorizeForm>,
) -> Result<Response, Response> {
    let result = mcp_authorize_consent_inner(state, headers, form).await;
    log_mcp_oauth_result("consent", &result);
    result
}

async fn mcp_authorize_consent_inner(
    state: ApiState,
    headers: HeaderMap,
    form: McpAuthorizeForm,
) -> Result<Response, Response> {
    let endpoint = mcp_endpoint(&state).map_err(|error| error.into_response())?;
    let actor = authenticated_form_actor(&headers, &state, &form.csrf_token)
        .await
        .map_err(|error| error.into_response())?;
    let state_value = form.state.as_deref().filter(|value| !value.is_empty());
    let request = McpAuthorizationRequest {
        client_id: form.client_id,
        redirect_uri: form.redirect_uri,
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
            &endpoint.authorization_server_uri,
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
                &endpoint.authorization_server_uri,
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
    issuer: &str,
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
    oauth_redirect_error(issuer, redirect_uri, state, code)
}

fn oauth_redirect_error(
    issuer: &str,
    redirect_uri: &str,
    state: Option<&str>,
    error: &'static str,
) -> Response {
    oauth_redirect(issuer, redirect_uri, state, None, Some(error))
}

fn oauth_redirect(
    issuer: &str,
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
        pairs.append_pair("iss", issuer);
    }
    Redirect::to(url.as_str()).into_response()
}
