//! HTTP error responseへの変換。

use axum::{Json, http::StatusCode};
use marginalis_application::{
    AuthenticationUseCaseError, NoteAdvisoryDiagnostic, NoteAdvisorySeverity, NoteUseCaseError,
    NoteValidationDiagnostic,
};
use marginalis_contract::{
    DiagnosticSeverityResponse, NoteDiagnosticResponse, ProblemCode, ProblemResponse,
    Utf8ByteSpanResponse, Utf8ByteUnit,
};
use marginalis_domain::Utf8ByteSpan;

fn span_response(span: Utf8ByteSpan) -> Utf8ByteSpanResponse {
    Utf8ByteSpanResponse {
        start: span.start,
        end: span.end,
        unit: Utf8ByteUnit::Utf8Byte,
    }
}

pub(super) fn advisory_response(diagnostic: NoteAdvisoryDiagnostic) -> NoteDiagnosticResponse {
    NoteDiagnosticResponse {
        code: diagnostic.code,
        // 保存を拒否しない指摘は`error`になり得ないため、公開表現の一部だけを使用する。
        severity: match diagnostic.severity {
            NoteAdvisorySeverity::Warning => DiagnosticSeverityResponse::Warning,
            NoteAdvisorySeverity::Information => DiagnosticSeverityResponse::Information,
            NoteAdvisorySeverity::Hint => DiagnosticSeverityResponse::Hint,
        },
        target: diagnostic.target,
        span: diagnostic.span.map(span_response),
        message: diagnostic.message,
    }
}

fn validation_response(diagnostic: NoteValidationDiagnostic) -> NoteDiagnosticResponse {
    NoteDiagnosticResponse {
        code: diagnostic.code,
        severity: DiagnosticSeverityResponse::Error,
        target: diagnostic.target,
        span: diagnostic.span.map(span_response),
        message: diagnostic.message,
    }
}

pub(super) type HandlerResult<T> = Result<T, (StatusCode, Json<ProblemResponse>)>;

pub(super) fn problem(
    status: StatusCode,
    code: ProblemCode,
    message: &'static str,
) -> (StatusCode, Json<ProblemResponse>) {
    record_problem_code(code);
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
        NoteUseCaseError::Conflict => problem(
            StatusCode::CONFLICT,
            ProblemCode::Conflict,
            "note revision conflicts",
        ),
        NoteUseCaseError::Validation(diagnostics) => {
            record_problem_code(ProblemCode::ValidationFailed);
            (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(validation_problem(diagnostics)),
            )
        }
        NoteUseCaseError::AdvisoriesRejected(diagnostics) => {
            record_problem_code(ProblemCode::ValidationFailed);
            (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(advisory_problem(diagnostics)),
            )
        }
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

fn advisory_problem(diagnostics: Vec<NoteAdvisoryDiagnostic>) -> ProblemResponse {
    ProblemResponse {
        code: ProblemCode::ValidationFailed,
        message: "note input contains warnings".into(),
        diagnostics: diagnostics.into_iter().map(advisory_response).collect(),
    }
}

fn record_problem_code(code: ProblemCode) {
    tracing::Span::current().record("problem_code", code.as_str());
}

fn validation_problem(diagnostics: Vec<NoteValidationDiagnostic>) -> ProblemResponse {
    ProblemResponse {
        code: ProblemCode::ValidationFailed,
        message: "note input is invalid".into(),
        diagnostics: diagnostics.into_iter().map(validation_response).collect(),
    }
}

pub(super) fn validation_problem_json(
    diagnostics: Vec<NoteValidationDiagnostic>,
) -> serde_json::Value {
    serde_json::to_value(validation_problem(diagnostics))
        .expect("validation problem is serializable")
}

pub(super) fn advisory_problem_json(diagnostics: Vec<NoteAdvisoryDiagnostic>) -> serde_json::Value {
    serde_json::to_value(advisory_problem(diagnostics)).expect("advisory problem is serializable")
}

pub(super) fn authentication_error(
    error: AuthenticationUseCaseError,
) -> (StatusCode, Json<ProblemResponse>) {
    match error {
        AuthenticationUseCaseError::Rejected => problem(
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
