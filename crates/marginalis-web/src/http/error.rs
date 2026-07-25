//! HTTP error responseへの変換。

use axum::{Json, http::StatusCode};
use marginalis_application::{AuthenticationUseCaseError, McpOAuthUseCaseError, NoteUseCaseError};
use serde::Serialize;

#[derive(Serialize)]
pub(super) struct Problem {
    code: &'static str,
    message: &'static str,
}

pub(super) type HandlerResult<T> = Result<T, (StatusCode, Json<Problem>)>;

pub(super) fn problem(
    status: StatusCode,
    code: &'static str,
    message: &'static str,
) -> (StatusCode, Json<Problem>) {
    (status, Json(Problem { code, message }))
}

pub(super) fn note_error(error: NoteUseCaseError) -> (StatusCode, Json<Problem>) {
    match error {
        NoteUseCaseError::NotFound => {
            problem(StatusCode::NOT_FOUND, "not_found", "note is not available")
        }
        NoteUseCaseError::Forbidden => problem(
            StatusCode::FORBIDDEN,
            "forbidden",
            "note operation is not permitted",
        ),
        NoteUseCaseError::Conflict => {
            problem(StatusCode::CONFLICT, "conflict", "note revision conflicts")
        }
        NoteUseCaseError::Validation => problem(
            StatusCode::UNPROCESSABLE_ENTITY,
            "validation_failed",
            "note is invalid",
        ),
        NoteUseCaseError::Unavailable => problem(
            StatusCode::SERVICE_UNAVAILABLE,
            "unavailable",
            "note operation is unavailable",
        ),
    }
}

pub(super) fn authentication_error(
    error: AuthenticationUseCaseError,
) -> (StatusCode, Json<Problem>) {
    match error {
        AuthenticationUseCaseError::Rejected | AuthenticationUseCaseError::NotFound => problem(
            StatusCode::UNAUTHORIZED,
            "authentication_required",
            "authentication is required",
        ),
        AuthenticationUseCaseError::Unavailable => problem(
            StatusCode::SERVICE_UNAVAILABLE,
            "authentication_unavailable",
            "authentication is unavailable",
        ),
    }
}

pub(super) fn mcp_error(error: McpOAuthUseCaseError) -> (StatusCode, Json<Problem>) {
    match error {
        McpOAuthUseCaseError::InvalidRequest
        | McpOAuthUseCaseError::InvalidClient
        | McpOAuthUseCaseError::InvalidRedirectUri
        | McpOAuthUseCaseError::InvalidScope
        | McpOAuthUseCaseError::InvalidTarget
        | McpOAuthUseCaseError::InvalidGrant => problem(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "OAuth request is invalid",
        ),
        McpOAuthUseCaseError::Unavailable => problem(
            StatusCode::SERVICE_UNAVAILABLE,
            "unavailable",
            "OAuth service is unavailable",
        ),
    }
}
