//! HTTP error responseへの変換。

use axum::{Json, http::StatusCode};
use marginalis_application::{
    AuthenticationUseCaseError, McpOAuthUseCaseError, NoteUseCaseError, NoteValidationDiagnostic,
    NoteValidationTarget,
};
use marginalis_contract::{
    ProblemCode, ProblemResponse, Utf8ByteSpanResponse, Utf8ByteUnit, ValidationDiagnosticResponse,
    ValidationTargetResponse,
};

fn diagnostic_response(diagnostic: NoteValidationDiagnostic) -> ValidationDiagnosticResponse {
    let target = match diagnostic.target {
        NoteValidationTarget::Title => ValidationTargetResponse::Title,
        NoteValidationTarget::Body => ValidationTargetResponse::Body,
        NoteValidationTarget::Tag { index } => ValidationTargetResponse::Tag { index },
        NoteValidationTarget::Tags => ValidationTargetResponse::Tags,
        NoteValidationTarget::AclEntry { index } => ValidationTargetResponse::AclEntry { index },
    };
    ValidationDiagnosticResponse {
        code: diagnostic.code.as_str().into(),
        target,
        span: diagnostic.span.map(|span| Utf8ByteSpanResponse {
            start: span.start,
            end: span.end,
            unit: Utf8ByteUnit::Utf8Byte,
        }),
        message: diagnostic.message.into(),
    }
}

pub(super) type HandlerResult<T> = Result<T, (StatusCode, Json<ProblemResponse>)>;

pub(super) fn problem(
    status: StatusCode,
    code: ProblemCode,
    message: &'static str,
) -> (StatusCode, Json<ProblemResponse>) {
    (
        status,
        Json(ProblemResponse {
            code,
            message: message.into(),
            diagnostics: Vec::new(),
        }),
    )
}

pub(super) fn note_error(error: NoteUseCaseError) -> (StatusCode, Json<ProblemResponse>) {
    match error {
        NoteUseCaseError::NotFound => problem(
            StatusCode::NOT_FOUND,
            ProblemCode::NotFound,
            "note is not available",
        ),
        NoteUseCaseError::Forbidden => problem(
            StatusCode::FORBIDDEN,
            ProblemCode::Forbidden,
            "note operation is not permitted",
        ),
        NoteUseCaseError::Conflict => problem(
            StatusCode::CONFLICT,
            ProblemCode::Conflict,
            "note revision conflicts",
        ),
        NoteUseCaseError::Validation(diagnostics) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(validation_problem(diagnostics)),
        ),
        NoteUseCaseError::RenderFailed => problem(
            StatusCode::UNPROCESSABLE_ENTITY,
            ProblemCode::RenderFailed,
            "note cannot be rendered safely",
        ),
        NoteUseCaseError::Unavailable => problem(
            StatusCode::SERVICE_UNAVAILABLE,
            ProblemCode::Unavailable,
            "note operation is unavailable",
        ),
    }
}

fn validation_problem(diagnostics: Vec<NoteValidationDiagnostic>) -> ProblemResponse {
    ProblemResponse {
        code: ProblemCode::ValidationFailed,
        message: "note input is invalid".into(),
        diagnostics: diagnostics.into_iter().map(diagnostic_response).collect(),
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
) -> (StatusCode, Json<ProblemResponse>) {
    match error {
        AuthenticationUseCaseError::Rejected | AuthenticationUseCaseError::NotFound => problem(
            StatusCode::UNAUTHORIZED,
            ProblemCode::AuthenticationRequired,
            "authentication is required",
        ),
        AuthenticationUseCaseError::Unavailable => problem(
            StatusCode::SERVICE_UNAVAILABLE,
            ProblemCode::AuthenticationUnavailable,
            "authentication is unavailable",
        ),
    }
}

pub(super) fn mcp_error(error: McpOAuthUseCaseError) -> (StatusCode, Json<ProblemResponse>) {
    match error {
        McpOAuthUseCaseError::InvalidRequest
        | McpOAuthUseCaseError::InvalidClient
        | McpOAuthUseCaseError::InvalidRedirectUri
        | McpOAuthUseCaseError::InvalidScope
        | McpOAuthUseCaseError::InvalidTarget
        | McpOAuthUseCaseError::InvalidGrant => problem(
            StatusCode::BAD_REQUEST,
            ProblemCode::InvalidRequest,
            "OAuth request is invalid",
        ),
        McpOAuthUseCaseError::Unavailable => problem(
            StatusCode::SERVICE_UNAVAILABLE,
            ProblemCode::Unavailable,
            "OAuth service is unavailable",
        ),
    }
}
