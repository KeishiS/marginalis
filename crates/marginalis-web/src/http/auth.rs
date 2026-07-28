//! OIDC login、Cookie session、CSRFとsame-origin検証。

use std::str::FromStr;

use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Redirect, Response},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use marginalis_application::AuthenticationUseCaseError;
use marginalis_contract::ProblemCode;
use marginalis_domain::{Actor, EntityId, NoteId};
use serde::Deserialize;

use super::{
    error::{HandlerResult, authentication_error, problem},
    state::ApiState,
};

const SESSION_COOKIE: &str = "marginalis_session";
pub(super) const CSRF_COOKIE: &str = "marginalis_csrf";
pub(super) const RETURN_TO_COOKIE: &str = "marginalis_return_to";

#[derive(Deserialize)]
pub(super) struct OidcCallbackQuery {
    code: String,
    state: String,
}

#[derive(Deserialize)]
pub(super) struct LoginQuery {
    next: Option<String>,
}

pub(super) fn external_path(base_path: &str, path: &str) -> String {
    debug_assert!(path.starts_with('/'));
    if base_path == "/" {
        path.into()
    } else {
        format!("{}{path}", base_path.trim_end_matches('/'))
    }
}

pub(super) fn valid_return_to(value: &str, base_path: &str) -> bool {
    let base_path = base_path.trim_end_matches('/');
    (value == base_path || value.starts_with(&format!("{base_path}/")))
        && value.starts_with('/')
        && !value.starts_with("//")
        && !value.contains('\r')
        && !value.contains('\n')
}

pub(super) async fn authenticated_ui_actor(
    headers: &HeaderMap,
    state: &ApiState,
    return_to: &str,
) -> Result<Actor, Response> {
    let Some(session_id) = cookie_value(headers, SESSION_COOKIE) else {
        return Err(login_redirect(state, return_to));
    };
    match state.sessions.authenticate_session(session_id).await {
        Ok(Some(session)) => Ok(session.actor),
        Ok(None)
        | Err(AuthenticationUseCaseError::Rejected | AuthenticationUseCaseError::NotFound) => {
            Err(login_redirect(state, return_to))
        }
        Err(AuthenticationUseCaseError::Unavailable) => {
            Err(authentication_error(AuthenticationUseCaseError::Unavailable).into_response())
        }
    }
}

fn login_redirect(state: &ApiState, return_to: &str) -> Response {
    let query = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("next", return_to)
        .finish();
    Redirect::temporary(&format!(
        "{}?{query}",
        external_path(&state.cookie_path, "/auth/oidc/login")
    ))
    .into_response()
}

pub(super) async fn begin_login(
    State(state): State<ApiState>,
    Query(query): Query<LoginQuery>,
) -> HandlerResult<Response> {
    let mut response = Redirect::temporary(
        &state
            .oidc
            .begin_login()
            .await
            .map_err(authentication_error)?,
    )
    .into_response();
    if let Some(next) = query
        .next
        .filter(|next| valid_return_to(next, &state.cookie_path))
    {
        let encoded = URL_SAFE_NO_PAD.encode(next);
        response.headers_mut().append(
            header::SET_COOKIE,
            format!(
                "{RETURN_TO_COOKIE}={encoded}; Path={}; Secure; HttpOnly; SameSite=Lax; Max-Age=600",
                state.cookie_path
            )
            .parse()
            .map_err(|_| {
                problem(
                    StatusCode::SERVICE_UNAVAILABLE,
                    ProblemCode::Unavailable,
                    "authentication is unavailable",
                )
            })?,
        );
    }
    Ok(response)
}

pub(super) async fn complete_login(
    State(state): State<ApiState>,
    headers: HeaderMap,
    axum::extract::Query(query): axum::extract::Query<OidcCallbackQuery>,
) -> HandlerResult<Response> {
    let actor = state
        .oidc
        .complete_login(query.code, query.state)
        .await
        .map_err(authentication_error)?;
    let session = state
        .sessions
        .issue_session(actor)
        .await
        .map_err(authentication_error)?;
    let return_to = cookie_value(&headers, RETURN_TO_COOKIE)
        .and_then(|value| URL_SAFE_NO_PAD.decode(value).ok())
        .and_then(|value| String::from_utf8(value).ok())
        .filter(|value| valid_return_to(value, &state.cookie_path))
        .unwrap_or_else(|| state.cookie_path.clone());
    let mut response = Redirect::to(&return_to).into_response();
    for value in [
        format!(
            "{SESSION_COOKIE}={}; Path={}; Secure; HttpOnly; SameSite=Lax",
            session.session_id, state.cookie_path
        ),
        format!(
            "{CSRF_COOKIE}={}; Path={}; Secure; SameSite=Lax",
            session.csrf_token, state.cookie_path
        ),
        format!(
            "{RETURN_TO_COOKIE}=; Path={}; Secure; HttpOnly; SameSite=Lax; Max-Age=0",
            state.cookie_path
        ),
    ] {
        response.headers_mut().append(
            header::SET_COOKIE,
            value.parse().map_err(|_| {
                problem(
                    StatusCode::SERVICE_UNAVAILABLE,
                    ProblemCode::Unavailable,
                    "authentication is unavailable",
                )
            })?,
        );
    }
    Ok(response)
}

pub(super) async fn logout(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> HandlerResult<Response> {
    let _actor = authenticated_mutation_actor(&headers, &state).await?;
    let session_id =
        cookie_value(&headers, SESSION_COOKIE).expect("authenticated session cookie exists");
    state
        .sessions
        .revoke_session(session_id)
        .await
        .map_err(authentication_error)?;
    let mut response = StatusCode::NO_CONTENT.into_response();
    for value in [
        format!(
            "{SESSION_COOKIE}=; Path={}; Max-Age=0; Secure; HttpOnly; SameSite=Lax",
            state.cookie_path
        ),
        format!(
            "{CSRF_COOKIE}=; Path={}; Max-Age=0; Secure; SameSite=Lax",
            state.cookie_path
        ),
    ] {
        response.headers_mut().append(
            header::SET_COOKIE,
            value.parse().map_err(|_| {
                problem(
                    StatusCode::SERVICE_UNAVAILABLE,
                    ProblemCode::Unavailable,
                    "authentication is unavailable",
                )
            })?,
        );
    }
    Ok(response)
}

pub(super) async fn authenticated_actor(
    headers: &HeaderMap,
    state: &ApiState,
) -> HandlerResult<Actor> {
    let session_id = cookie_value(headers, SESSION_COOKIE).ok_or_else(|| {
        problem(
            StatusCode::UNAUTHORIZED,
            ProblemCode::AuthenticationRequired,
            "authentication is required",
        )
    })?;
    state
        .sessions
        .authenticate_session(session_id)
        .await
        .map_err(authentication_error)?
        .map(|session| session.actor)
        .ok_or_else(|| {
            problem(
                StatusCode::UNAUTHORIZED,
                ProblemCode::AuthenticationRequired,
                "authentication is required",
            )
        })
}

pub(super) async fn authenticated_mutation_actor(
    headers: &HeaderMap,
    state: &ApiState,
) -> HandlerResult<Actor> {
    let actor = authenticated_actor(headers, state).await?;
    validate_mutation_origin(headers, state)?;
    let session_id =
        cookie_value(headers, SESSION_COOKIE).expect("authenticated session cookie exists");
    let csrf_cookie = cookie_value(headers, CSRF_COOKIE).ok_or_else(|| {
        problem(
            StatusCode::FORBIDDEN,
            ProblemCode::CsrfRequired,
            "CSRF token is required",
        )
    })?;
    let csrf_header = headers
        .get("x-csrf-token")
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| {
            problem(
                StatusCode::FORBIDDEN,
                ProblemCode::CsrfRequired,
                "CSRF token is required",
            )
        })?;
    if csrf_cookie != csrf_header
        || !state
            .sessions
            .verify_csrf(session_id, csrf_header.into())
            .await
            .map_err(authentication_error)?
    {
        return Err(problem(
            StatusCode::FORBIDDEN,
            ProblemCode::CsrfInvalid,
            "CSRF token is invalid",
        ));
    }
    Ok(actor)
}

pub(super) fn validate_mutation_origin(headers: &HeaderMap, state: &ApiState) -> HandlerResult<()> {
    let received_origin = headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok());
    if received_origin != Some(state.browser_origin.as_str()) {
        tracing::warn!(
            received_origin = ?received_origin,
            expected_origin = %state.browser_origin,
            "rejected browser mutation with a missing or mismatched origin"
        );
        return Err(problem(
            StatusCode::FORBIDDEN,
            ProblemCode::SameOriginRequired,
            "same-origin request is required",
        ));
    }
    if let Some(site) = headers
        .get("sec-fetch-site")
        .and_then(|value| value.to_str().ok())
        && !matches!(site, "same-origin" | "none")
    {
        tracing::warn!(
            sec_fetch_site = site,
            "rejected browser mutation with cross-site fetch metadata"
        );
        return Err(problem(
            StatusCode::FORBIDDEN,
            ProblemCode::SameOriginRequired,
            "same-origin request is required",
        ));
    }
    Ok(())
}

/// OAuth consent may run in a client-controlled popup or sandbox whose `Origin` is absent or
/// opaque. Such requests rely on the session-bound double-submit token. A concrete origin must
/// still match Marginalis so that a foreign site cannot submit a copied form.
pub(super) async fn authenticated_form_actor(
    headers: &HeaderMap,
    state: &ApiState,
    csrf_token: &str,
) -> HandlerResult<Actor> {
    let actor = authenticated_actor(headers, state).await?;
    if let Some(origin) = headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
        && origin != "null"
        && origin != state.browser_origin
    {
        tracing::warn!(
            received_origin = origin,
            expected_origin = %state.browser_origin,
            "rejected OAuth consent with a mismatched concrete origin"
        );
        return Err(problem(
            StatusCode::FORBIDDEN,
            ProblemCode::SameOriginRequired,
            "same-origin request is required",
        ));
    }
    let session_id =
        cookie_value(headers, SESSION_COOKIE).expect("authenticated session cookie exists");
    if cookie_value(headers, CSRF_COOKIE).as_deref() != Some(csrf_token)
        || !state
            .sessions
            .verify_csrf(session_id, csrf_token.into())
            .await
            .map_err(authentication_error)?
    {
        return Err(problem(
            StatusCode::FORBIDDEN,
            ProblemCode::CsrfInvalid,
            "CSRF token is invalid",
        ));
    }
    Ok(actor)
}

pub(super) fn parse_note_id(value: &str) -> HandlerResult<NoteId> {
    EntityId::from_str(value).map(NoteId::new).map_err(|_| {
        problem(
            StatusCode::NOT_FOUND,
            ProblemCode::NotFound,
            "note is not available",
        )
    })
}

pub(super) fn cookie_value(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .find_map(|part| {
            let (key, value) = part.trim().split_once('=')?;
            (key == name).then(|| value.to_owned())
        })
}
