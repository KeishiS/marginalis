//! HTTP error responseへの変換。

use axum::{Json, http::StatusCode};
use marginalis_application::{
    AuthenticationUseCaseError, BibliographyUseCaseError, MathMacroUseCaseError,
    NoteAdvisoryDiagnostic, NoteAdvisorySeverity, NoteUseCaseError, NoteValidationDiagnostic,
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

/// ノート操作の失敗を公開エラー表現へ写像する唯一の関数。
///
/// RESTとMCPはこの結果だけを使い、transportごとに`code`と`message`を組み立てない。
/// 同じ失敗が接続方法によって別の応答になることを防ぐ。
pub(super) fn note_problem(error: NoteUseCaseError) -> ProblemResponse {
    match error {
        NoteUseCaseError::NotFound => {
            ProblemResponse::new(ProblemCode::NotFound, "note is not available")
        }
        NoteUseCaseError::Conflict => {
            ProblemResponse::new(ProblemCode::Conflict, "note revision conflicts")
        }
        NoteUseCaseError::Validation(diagnostics) => ProblemResponse {
            code: ProblemCode::ValidationFailed,
            message: "note input is invalid".into(),
            diagnostics: diagnostics.into_iter().map(validation_response).collect(),
        },
        NoteUseCaseError::AdvisoriesRejected(diagnostics) => ProblemResponse {
            code: ProblemCode::ValidationFailed,
            message: "note input contains warnings".into(),
            diagnostics: diagnostics.into_iter().map(advisory_response).collect(),
        },
        NoteUseCaseError::RenderFailed => {
            ProblemResponse::new(ProblemCode::RenderFailed, "note cannot be rendered safely")
        }
        // 保存内容の破損は運用上は一時障害と区別するが、内部状態を開示しないため
        // 利用者向けの応答は同じにする。
        NoteUseCaseError::Unavailable | NoteUseCaseError::CorruptData => {
            ProblemResponse::new(ProblemCode::Unavailable, "note operation is unavailable")
        }
    }
}

/// 書誌ライブラリー操作の失敗を公開エラー表現へ写像する唯一の関数。
pub(super) fn bibliography_problem(error: BibliographyUseCaseError) -> ProblemResponse {
    match error {
        BibliographyUseCaseError::InvalidSearchQuery => ProblemResponse::new(
            ProblemCode::InvalidRequest,
            "bibliography search query is invalid",
        ),
        BibliographyUseCaseError::InvalidCslJson => ProblemResponse::new(
            ProblemCode::ValidationFailed,
            "CSL-JSON must contain valid id and type fields",
        ),
        BibliographyUseCaseError::NotFound => {
            ProblemResponse::new(ProblemCode::NotFound, "bibliography item was not found")
        }
        BibliographyUseCaseError::Conflict => ProblemResponse::new(
            ProblemCode::Conflict,
            "citation key already exists or revision conflicts",
        ),
        BibliographyUseCaseError::Unavailable | BibliographyUseCaseError::CorruptData => {
            ProblemResponse::new(
                ProblemCode::Unavailable,
                "bibliography service is unavailable",
            )
        }
    }
}

/// 公開エラーcodeに対応するHTTP status。RESTだけが使用する。
pub(super) const fn problem_status(code: ProblemCode) -> StatusCode {
    match code {
        ProblemCode::AuthenticationRequired => StatusCode::UNAUTHORIZED,
        ProblemCode::AuthenticationUnavailable | ProblemCode::Unavailable => {
            StatusCode::SERVICE_UNAVAILABLE
        }
        ProblemCode::CsrfRejected
        | ProblemCode::CsrfRequired
        | ProblemCode::CsrfInvalid
        | ProblemCode::SameOriginRequired
        | ProblemCode::OriginNotAllowed
        | ProblemCode::Forbidden => StatusCode::FORBIDDEN,
        ProblemCode::NotFound => StatusCode::NOT_FOUND,
        ProblemCode::Conflict => StatusCode::CONFLICT,
        ProblemCode::PreconditionRequired => StatusCode::PRECONDITION_REQUIRED,
        ProblemCode::InvalidRequest => StatusCode::BAD_REQUEST,
        ProblemCode::ValidationFailed | ProblemCode::RenderFailed => {
            StatusCode::UNPROCESSABLE_ENTITY
        }
    }
}

fn problem_response(problem: ProblemResponse) -> (StatusCode, Json<ProblemResponse>) {
    record_problem_code(problem.code);
    (problem_status(problem.code), Json(problem))
}

pub(super) fn note_error(error: NoteUseCaseError) -> (StatusCode, Json<ProblemResponse>) {
    problem_response(note_problem(error))
}

pub(super) fn bibliography_error(
    error: BibliographyUseCaseError,
) -> (StatusCode, Json<ProblemResponse>) {
    problem_response(bibliography_problem(error))
}

pub(super) fn math_macro_error(
    error: MathMacroUseCaseError,
) -> (StatusCode, Json<ProblemResponse>) {
    problem_response(match error {
        MathMacroUseCaseError::Invalid => ProblemResponse::new(
            ProblemCode::ValidationFailed,
            "MathJax macro settings are invalid",
        ),
        MathMacroUseCaseError::Conflict => ProblemResponse::new(
            ProblemCode::Conflict,
            "MathJax macro settings revision conflicts",
        ),
        MathMacroUseCaseError::Unavailable | MathMacroUseCaseError::CorruptData => {
            ProblemResponse::new(
                ProblemCode::Unavailable,
                "MathJax macro settings are unavailable",
            )
        }
    })
}

fn record_problem_code(code: ProblemCode) {
    tracing::Span::current().record("problem_code", code.as_str());
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
