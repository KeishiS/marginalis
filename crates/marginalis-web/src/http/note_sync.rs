//! 外部検索用コピーへノートを反映するOAuth保護REST API。

use axum::{
    Json,
    extract::{Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use marginalis_application::{NoteSyncEntry, NoteSyncPhase, NoteSyncRemovalReason};
use marginalis_contract::{
    NoteSyncEntryResponse, NoteSyncPageResponse, NoteSyncPhaseResponse,
    NoteSyncRemovalReasonResponse, ProblemCode,
};
use serde::Deserialize;

use super::{
    error::{HandlerResult, note_error, problem},
    notes::note_response,
    resource_authorization::{BearerToken, authenticate, authentication_challenge, bearer_token},
    state::ApiState,
};

#[derive(Clone, Debug, Default, Deserialize)]
pub(super) struct NoteSyncQuery {
    cursor: Option<String>,
    limit: Option<usize>,
}

pub(super) async fn sync_notes(
    State(state): State<ApiState>,
    Query(query): Query<NoteSyncQuery>,
    headers: HeaderMap,
) -> HandlerResult<Response> {
    let endpoint = state.mcp.as_ref().ok_or_else(|| {
        problem(
            StatusCode::SERVICE_UNAVAILABLE,
            ProblemCode::Unavailable,
            "OAuth synchronization is unavailable",
        )
    })?;
    let token = match bearer_token(&headers) {
        BearerToken::Value(token) => token,
        BearerToken::Missing => {
            tracing::warn!(
                event = "note_sync.authentication.failed",
                reason = "missing-token",
                "note synchronization access token is missing"
            );
            return Ok(rest_authentication_challenge(
                endpoint,
                StatusCode::UNAUTHORIZED,
                None,
                "notes:sync",
                ProblemCode::AuthenticationRequired,
                "OAuth access token is required",
            ));
        }
        BearerToken::Malformed => {
            tracing::warn!(
                event = "note_sync.authentication.failed",
                reason = "token-format",
                "note synchronization authorization header is malformed"
            );
            return Ok(rest_authentication_challenge(
                endpoint,
                StatusCode::UNAUTHORIZED,
                Some("invalid_token"),
                "notes:sync",
                ProblemCode::AuthenticationRequired,
                "OAuth access token is invalid",
            ));
        }
    };
    let authenticated = match authenticate(endpoint, token, &[&["notes:sync"]]).await? {
        Ok(authenticated) => authenticated,
        Err(response) => return Ok(rest_authentication_response(response)),
    };
    let page = state
        .notes
        .sync_notes(authenticated.actor, query.cursor, query.limit)
        .await
        .map_err(note_error)?;

    Ok(Json(NoteSyncPageResponse {
        phase: match page.phase {
            NoteSyncPhase::Snapshot => NoteSyncPhaseResponse::Snapshot,
            NoteSyncPhase::Changes => NoteSyncPhaseResponse::Changes,
        },
        entries: page
            .entries
            .into_iter()
            .map(|entry| match entry {
                NoteSyncEntry::Upsert(note) => NoteSyncEntryResponse::Upsert {
                    note: note_response(*note),
                },
                NoteSyncEntry::Remove { note_id, reason } => NoteSyncEntryResponse::Remove {
                    note_id: note_id.to_string(),
                    reason: match reason {
                        NoteSyncRemovalReason::Deleted => NoteSyncRemovalReasonResponse::Deleted,
                        NoteSyncRemovalReason::AccessRevoked => {
                            NoteSyncRemovalReasonResponse::AccessRevoked
                        }
                    },
                },
            })
            .collect(),
        next_cursor: page.next_cursor,
        has_more: page.has_more,
        cursor_expires_at_ms: page.cursor_expires_at.get(),
    })
    .into_response())
}

fn rest_authentication_response(response: Response) -> Response {
    let (code, message) = match response.status() {
        StatusCode::UNAUTHORIZED => (
            ProblemCode::AuthenticationRequired,
            "OAuth access token is invalid",
        ),
        StatusCode::FORBIDDEN => (
            ProblemCode::Forbidden,
            "OAuth access token does not grant notes:sync",
        ),
        _ => return response,
    };
    let mut rest = problem(response.status(), code, message).into_response();
    if let Some(value) = response.headers().get(header::WWW_AUTHENTICATE) {
        rest.headers_mut()
            .insert(header::WWW_AUTHENTICATE, value.clone());
    }
    rest
}

fn rest_authentication_challenge(
    endpoint: &super::state::McpEndpoint,
    status: StatusCode,
    error: Option<&str>,
    scope: &str,
    code: ProblemCode,
    message: &'static str,
) -> Response {
    let challenge = authentication_challenge(endpoint, status, error, scope);
    let mut response = problem(status, code, message).into_response();
    if let Some(value) = challenge.headers().get(header::WWW_AUTHENTICATE) {
        response
            .headers_mut()
            .insert(header::WWW_AUTHENTICATE, value.clone());
    }
    response
}
