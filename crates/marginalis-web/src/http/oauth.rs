//! Marginalisが提供するMCP OAuth authorization server境界。

use std::time::Instant;

use axum::{
    Form, Json,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{Html, IntoResponse, Redirect, Response},
};
use marginalis_application::{McpAuthorizationRequest, McpOAuthUseCaseError};
use serde::{Deserialize, Serialize};

use super::{
    auth::{
        CSRF_COOKIE, authenticated_actor, authenticated_form_actor, authenticated_mutation_actor,
        cookie_value, external_path,
    },
    error::{HandlerResult, mcp_error, problem},
    html::escape_html,
    mcp_endpoint,
    state::ApiState,
};

pub(super) async fn mcp_resource_metadata(
    State(state): State<ApiState>,
) -> HandlerResult<Json<serde_json::Value>> {
    let endpoint = mcp_endpoint(&state)?;
    Ok(Json(
        serde_json::json!({"resource": endpoint.resource_uri, "authorization_servers": [endpoint.authorization_server_uri], "bearer_methods_supported": ["header"], "scopes_supported": ["notes:read", "notes:write", "notes:delete"]}),
    ))
}

pub(super) async fn mcp_server_metadata(
    State(state): State<ApiState>,
) -> HandlerResult<Json<serde_json::Value>> {
    let endpoint = mcp_endpoint(&state)?;
    Ok(Json(
        serde_json::json!({"issuer": endpoint.authorization_server_uri, "authorization_endpoint": endpoint.authorization_endpoint_uri, "token_endpoint": endpoint.token_endpoint_uri, "registration_endpoint": format!("{}/oauth/register", endpoint.authorization_server_uri.trim_end_matches('/')), "response_types_supported": ["code"], "grant_types_supported": ["authorization_code", "refresh_token"], "code_challenge_methods_supported": ["S256"], "token_endpoint_auth_methods_supported": ["none"]}),
    ))
}

#[derive(Deserialize)]
pub(super) struct McpAuthorizeQuery {
    response_type: String,
    client_id: String,
    redirect_uri: String,
    resource: String,
    scope: String,
    code_challenge: String,
    code_challenge_method: String,
    state: Option<String>,
}

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

#[derive(Deserialize)]
pub(super) struct McpAuthorizePost {
    response_type: Option<String>,
    client_id: String,
    redirect_uri: String,
    resource: String,
    scope: String,
    code_challenge: String,
    code_challenge_method: Option<String>,
    state: Option<String>,
    csrf_token: Option<String>,
    decision: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct McpTokenForm {
    grant_type: String,
    code: Option<String>,
    client_id: String,
    redirect_uri: Option<String>,
    resource: String,
    code_verifier: Option<String>,
    refresh_token: Option<String>,
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
pub(super) struct McpRegistrationRequest {
    client_name: Option<String>,
    redirect_uris: Vec<String>,
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
    Json(request): Json<McpRegistrationRequest>,
) -> Result<Response, Response> {
    if !state.mcp_registration_limiter.allow(Instant::now()) {
        return Err(oauth_error_response(
            StatusCode::TOO_MANY_REQUESTS,
            "temporarily_unavailable",
            "dynamic client registration rate limit exceeded",
        ));
    }
    let endpoint = mcp_endpoint(&state).map_err(|error| error.into_response())?;
    let display_name = request
        .client_name
        .filter(|name| !name.trim().is_empty())
        .ok_or_else(|| {
            oauth_error_response(
                StatusCode::BAD_REQUEST,
                "invalid_client_metadata",
                "client_name is required",
            )
        })?;
    let client = marginalis_domain::McpOAuthClient {
        client_id: format!("mcp-{}", uuid::Uuid::now_v7()),
        display_name,
        redirect_uris: request.redirect_uris,
    };
    endpoint
        .oauth
        .register_client(client.clone())
        .await
        .map_err(|error| match error {
            McpOAuthUseCaseError::Rejected => oauth_error_response(
                StatusCode::BAD_REQUEST,
                "invalid_client_metadata",
                "client metadata is invalid",
            ),
            McpOAuthUseCaseError::Unavailable => oauth_error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                "temporarily_unavailable",
                "OAuth service is unavailable",
            ),
        })?;
    Ok((
        StatusCode::CREATED,
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

fn authorize_fields(query: &McpAuthorizeQuery) -> HandlerResult<(Vec<String>, String)> {
    if query.response_type != "code"
        || query.code_challenge_method != "S256"
        || query.code_challenge.is_empty()
    {
        return Err(problem(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "OAuth request is invalid",
        ));
    }
    let scopes = query
        .scope
        .split_ascii_whitespace()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if scopes.is_empty() {
        return Err(problem(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "OAuth request is invalid",
        ));
    }
    Ok((scopes, query.code_challenge.clone()))
}

fn authorization_request(query: &McpAuthorizeQuery) -> HandlerResult<McpAuthorizationRequest> {
    let (scopes, code_challenge) = authorize_fields(query)?;
    Ok(McpAuthorizationRequest {
        client_id: query.client_id.clone(),
        redirect_uri: query.redirect_uri.clone(),
        resource_uri: query.resource.clone(),
        scopes,
        code_challenge,
    })
}

pub(super) async fn mcp_authorize(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Query(query): Query<McpAuthorizeQuery>,
) -> HandlerResult<Response> {
    mcp_authorize_request(&state, &headers, query).await
}

async fn mcp_authorize_request(
    state: &ApiState,
    headers: &HeaderMap,
    query: McpAuthorizeQuery,
) -> HandlerResult<Response> {
    let endpoint = mcp_endpoint(state)?;
    let request = authorization_request(&query)?;
    let client = endpoint
        .oauth
        .validate_authorization_request(request)
        .await
        .map_err(mcp_error)?;
    let _actor = match authenticated_actor(headers, state).await {
        Ok(actor) => actor,
        Err((StatusCode::UNAUTHORIZED, _)) => {
            let mut request_uri = url::Url::parse("https://invalid.example/oauth/authorize")
                .expect("constant authorization URL is valid");
            {
                let mut pairs = request_uri.query_pairs_mut();
                pairs.append_pair("response_type", &query.response_type);
                pairs.append_pair("client_id", &query.client_id);
                pairs.append_pair("redirect_uri", &query.redirect_uri);
                pairs.append_pair("resource", &query.resource);
                pairs.append_pair("scope", &query.scope);
                pairs.append_pair("code_challenge", &query.code_challenge);
                pairs.append_pair("code_challenge_method", &query.code_challenge_method);
                if let Some(state) = &query.state {
                    pairs.append_pair("state", state);
                }
            }
            let next = format!(
                "{}?{}",
                external_path(&state.cookie_path, "/oauth/authorize"),
                request_uri.query().expect("query pairs were added")
            );
            let encoded_next =
                url::form_urlencoded::byte_serialize(next.as_bytes()).collect::<String>();
            return Ok(Redirect::to(&format!(
                "{}?next={encoded_next}",
                external_path(&state.cookie_path, "/auth/oidc/login")
            ))
            .into_response());
        }
        Err(error) => return Err(error),
    };
    let csrf = cookie_value(headers, CSRF_COOKIE).ok_or_else(|| {
        problem(
            StatusCode::FORBIDDEN,
            "csrf_required",
            "CSRF token is required",
        )
    })?;
    Ok(Html(format!(
        "<!doctype html><meta charset=\"utf-8\"><title>MCP authorization</title><main><h1>Authorize {}</h1><p>Requested scopes: {}</p><p>Redirect host: {}</p><form method=\"post\"><input type=\"hidden\" name=\"client_id\" value=\"{}\"><input type=\"hidden\" name=\"redirect_uri\" value=\"{}\"><input type=\"hidden\" name=\"resource\" value=\"{}\"><input type=\"hidden\" name=\"scope\" value=\"{}\"><input type=\"hidden\" name=\"code_challenge\" value=\"{}\"><input type=\"hidden\" name=\"state\" value=\"{}\"><input type=\"hidden\" name=\"csrf_token\" value=\"{}\"><button name=\"decision\" value=\"approve\">Allow</button><button name=\"decision\" value=\"deny\">Deny</button></form></main>",
        escape_html(&client.display_name),
        escape_html(&query.scope),
        escape_html(url::Url::parse(&query.redirect_uri).ok().and_then(|url| url.host_str().map(str::to_owned)).as_deref().unwrap_or("unknown")),
        escape_html(&query.client_id),
        escape_html(&query.redirect_uri),
        escape_html(&query.resource),
        escape_html(&query.scope),
        escape_html(&query.code_challenge),
        escape_html(query.state.as_deref().unwrap_or_default()),
        escape_html(&csrf)
    )).into_response())
}

fn required_authorize_value(value: Option<String>) -> HandlerResult<String> {
    value.ok_or_else(|| {
        problem(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "OAuth request is invalid",
        )
    })
}

/// OAuth clientからの初回POSTは状態を変更せず、検証後にloginまたは承認画面へ進める。
/// Marginalis自身の承認formだけはsession-bound CSRFとsame-origin検証を必須にする。
pub(super) async fn mcp_authorize_post(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Form(form): Form<McpAuthorizePost>,
) -> HandlerResult<Response> {
    if form.decision.is_some() || form.csrf_token.is_some() {
        let approval = McpAuthorizeForm {
            client_id: form.client_id,
            redirect_uri: form.redirect_uri,
            resource: form.resource,
            scope: form.scope,
            code_challenge: form.code_challenge,
            state: form.state,
            csrf_token: required_authorize_value(form.csrf_token)?,
            decision: required_authorize_value(form.decision)?,
        };
        return mcp_authorize_submit(&state, &headers, approval).await;
    }
    let request = McpAuthorizeQuery {
        response_type: required_authorize_value(form.response_type)?,
        client_id: form.client_id,
        redirect_uri: form.redirect_uri,
        resource: form.resource,
        scope: form.scope,
        code_challenge: form.code_challenge,
        code_challenge_method: required_authorize_value(form.code_challenge_method)?,
        state: form.state,
    };
    mcp_authorize_request(&state, &headers, request).await
}

async fn mcp_authorize_submit(
    state: &ApiState,
    headers: &HeaderMap,
    form: McpAuthorizeForm,
) -> HandlerResult<Response> {
    let endpoint = mcp_endpoint(state)?;
    let actor = authenticated_form_actor(headers, state, &form.csrf_token).await?;
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
    endpoint
        .oauth
        .validate_authorization_request(request.clone())
        .await
        .map_err(mcp_error)?;
    if form.decision != "approve" {
        return oauth_redirect(
            &request.redirect_uri,
            form.state.as_deref(),
            None,
            Some("access_denied"),
        );
    }
    let code = endpoint
        .oauth
        .authorize(actor, request.clone())
        .await
        .map_err(mcp_error)?;
    oauth_redirect(
        &request.redirect_uri,
        form.state.as_deref(),
        Some(&code),
        None,
    )
}

fn oauth_redirect(
    redirect_uri: &str,
    state: Option<&str>,
    code: Option<&str>,
    error: Option<&str>,
) -> HandlerResult<Response> {
    let mut url = url::Url::parse(redirect_uri).map_err(|_| {
        problem(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "redirect URI is invalid",
        )
    })?;
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
    Ok(Redirect::to(url.as_str()).into_response())
}

pub(super) async fn mcp_token(
    State(state): State<ApiState>,
    Form(form): Form<McpTokenForm>,
) -> Result<Response, Response> {
    let endpoint = mcp_endpoint(&state).map_err(|_| {
        oauth_error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "temporarily_unavailable",
            "OAuth service is unavailable",
        )
    })?;
    let pair = match form.grant_type.as_str() {
        "authorization_code" => endpoint
            .oauth
            .exchange_authorization_code(
                form.code.ok_or_else(|| {
                    oauth_error_response(
                        StatusCode::BAD_REQUEST,
                        "invalid_request",
                        "code is required",
                    )
                })?,
                form.client_id,
                form.redirect_uri.ok_or_else(|| {
                    oauth_error_response(
                        StatusCode::BAD_REQUEST,
                        "invalid_request",
                        "redirect_uri is required",
                    )
                })?,
                form.resource,
                form.code_verifier.ok_or_else(|| {
                    oauth_error_response(
                        StatusCode::BAD_REQUEST,
                        "invalid_request",
                        "code_verifier is required",
                    )
                })?,
            )
            .await
            .map_err(|error| match error {
                McpOAuthUseCaseError::Rejected => oauth_error_response(
                    StatusCode::BAD_REQUEST,
                    "invalid_grant",
                    "authorization code is invalid",
                ),
                McpOAuthUseCaseError::Unavailable => oauth_error_response(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "temporarily_unavailable",
                    "OAuth service is unavailable",
                ),
            })?,
        "refresh_token" => endpoint
            .oauth
            .refresh_access_token(
                form.refresh_token.ok_or_else(|| {
                    oauth_error_response(
                        StatusCode::BAD_REQUEST,
                        "invalid_request",
                        "refresh_token is required",
                    )
                })?,
                form.client_id,
                form.resource,
            )
            .await
            .map_err(|error| match error {
                McpOAuthUseCaseError::Rejected => oauth_error_response(
                    StatusCode::BAD_REQUEST,
                    "invalid_grant",
                    "refresh token is invalid",
                ),
                McpOAuthUseCaseError::Unavailable => oauth_error_response(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "temporarily_unavailable",
                    "OAuth service is unavailable",
                ),
            })?,
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
    endpoint
        .oauth
        .revoke(actor, client_id)
        .await
        .map_err(mcp_error)?;
    Ok(StatusCode::NO_CONTENT)
}
