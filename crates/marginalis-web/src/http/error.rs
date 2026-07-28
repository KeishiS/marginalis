//! HTTP error responseへの変換。

use axum::{Json, http::StatusCode};
use marginalis_application::{
    AuthenticationUseCaseError, McpOAuthUseCaseError, NoteUseCaseError, NoteValidationDiagnostic,
    NoteValidationTarget,
};
use serde::Serialize;

#[derive(Serialize)]
pub(super) struct Problem {
    code: &'static str,
    message: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    diagnostics: Option<Vec<ValidationDiagnosticResponse>>,
}

#[derive(Serialize)]
pub(super) struct ValidationDiagnosticResponse {
    code: &'static str,
    target: DiagnosticTargetResponse,
    #[serde(skip_serializing_if = "Option::is_none")]
    span: Option<Utf8ByteSpanResponse>,
    message: &'static str,
}

#[derive(Serialize)]
#[serde(tag = "field", rename_all = "snake_case")]
enum DiagnosticTargetResponse {
    Title,
    Body,
    Tag { index: usize },
    Tags,
    AclEntry { index: usize },
}

#[derive(Serialize)]
struct Utf8ByteSpanResponse {
    start: u32,
    end: u32,
    unit: &'static str,
}

impl From<NoteValidationDiagnostic> for ValidationDiagnosticResponse {
    fn from(diagnostic: NoteValidationDiagnostic) -> Self {
        let target = match diagnostic.target {
            NoteValidationTarget::Title => DiagnosticTargetResponse::Title,
            NoteValidationTarget::Body => DiagnosticTargetResponse::Body,
            NoteValidationTarget::Tag { index } => DiagnosticTargetResponse::Tag { index },
            NoteValidationTarget::Tags => DiagnosticTargetResponse::Tags,
            NoteValidationTarget::AclEntry { index } => {
                DiagnosticTargetResponse::AclEntry { index }
            }
        };
        Self {
            code: diagnostic.code.as_str(),
            target,
            span: diagnostic.span.map(|span| Utf8ByteSpanResponse {
                start: span.start,
                end: span.end,
                unit: "utf8_byte",
            }),
            message: diagnostic.message,
        }
    }
}

pub(super) type HandlerResult<T> = Result<T, (StatusCode, Json<Problem>)>;

pub(super) fn problem(
    status: StatusCode,
    code: &'static str,
    message: &'static str,
) -> (StatusCode, Json<Problem>) {
    (
        status,
        Json(Problem {
            code,
            message,
            diagnostics: None,
        }),
    )
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
        NoteUseCaseError::Validation(diagnostics) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(validation_problem(diagnostics)),
        ),
        NoteUseCaseError::Unavailable => problem(
            StatusCode::SERVICE_UNAVAILABLE,
            "unavailable",
            "note operation is unavailable",
        ),
    }
}

fn validation_problem(diagnostics: Vec<NoteValidationDiagnostic>) -> Problem {
    Problem {
        code: "validation_failed",
        message: "note input is invalid",
        diagnostics: Some(
            diagnostics
                .into_iter()
                .map(ValidationDiagnosticResponse::from)
                .collect(),
        ),
    }
}

pub(super) fn validation_problem_json(
    diagnostics: Vec<NoteValidationDiagnostic>,
) -> serde_json::Value {
    serde_json::to_value(validation_problem(diagnostics))
        .expect("validation problem is serializable")
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
