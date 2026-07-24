//! v0.3.0専用のHTTP APIと早期閲覧UI。
//!
//! このmoduleはv0.2の`/api/v1`・root管理・ローカル`UserId`を参照しない。composition rootは
//! v0.3.0ではこのrouterだけを公開する。

use std::{str::FromStr, sync::Arc};

use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode, header},
    response::{Html, IntoResponse, Redirect, Response},
    routing::get,
};
use marginalis_application::{
    AuthenticationUseCaseError, NoteUseCaseError, V3NoteUseCases, V3OidcAuthenticationUseCases,
    V3WebSessionUseCases,
};
use marginalis_domain::{CanonicalActor, CanonicalNote, CanonicalNoteDraft, EntityId, NoteId};
use serde::{Deserialize, Serialize};

pub const API_VERSION: &str = "v2";
pub const OPENAPI_DOCUMENT: &str = include_str!("../../../docs/openapi-v3.json");
const SESSION_COOKIE: &str = "marginalis_session";
const CSRF_COOKIE: &str = "marginalis_csrf";

#[derive(Clone)]
pub struct V3ApiState {
    pub notes: Arc<dyn V3NoteUseCases>,
    pub sessions: Arc<dyn V3WebSessionUseCases>,
    pub oidc: Arc<dyn V3OidcAuthenticationUseCases>,
    pub cookie_path: String,
    pub browser_origin: String,
}

impl V3ApiState {
    pub fn new(
        notes: Arc<dyn V3NoteUseCases>,
        sessions: Arc<dyn V3WebSessionUseCases>,
        oidc: Arc<dyn V3OidcAuthenticationUseCases>,
        cookie_path: String,
        browser_origin: String,
    ) -> Self {
        Self {
            notes,
            sessions,
            oidc,
            cookie_path,
            browser_origin,
        }
    }
}

#[derive(Serialize)]
struct Problem {
    code: &'static str,
    message: &'static str,
}

type V3Result<T> = Result<T, (StatusCode, Json<Problem>)>;

fn problem(
    status: StatusCode,
    code: &'static str,
    message: &'static str,
) -> (StatusCode, Json<Problem>) {
    (status, Json(Problem { code, message }))
}

fn note_error(error: NoteUseCaseError) -> (StatusCode, Json<Problem>) {
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

fn authentication_error(error: AuthenticationUseCaseError) -> (StatusCode, Json<Problem>) {
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

#[derive(Serialize)]
struct SessionResponse {
    issuer: String,
    subject: String,
    is_administrator: bool,
}

#[derive(Serialize)]
struct NoteResponse {
    note_id: String,
    title: String,
    body: String,
    tags: Vec<String>,
    created_at_ms: i64,
    updated_at_ms: i64,
    revision: i64,
}

impl From<CanonicalNote> for NoteResponse {
    fn from(note: CanonicalNote) -> Self {
        Self {
            note_id: note.note_id.to_string(),
            title: note.title,
            body: note.body,
            tags: note.tags,
            created_at_ms: note.created_at.get(),
            updated_at_ms: note.updated_at.get(),
            revision: note.revision,
        }
    }
}

#[derive(Deserialize)]
struct NoteInput {
    title: String,
    body: String,
    tags: Vec<String>,
}

#[derive(Deserialize)]
struct NoteUpdateInput {
    title: String,
    body: String,
    tags: Vec<String>,
    expected_revision: i64,
}

#[derive(Deserialize)]
struct DeleteInput {
    expected_revision: i64,
}

pub fn router(state: V3ApiState) -> Router {
    Router::new()
        .route("/", get(home))
        .route("/api/v2/openapi.json", get(openapi))
        .route("/auth/oidc/login", get(begin_login))
        .route("/auth/oidc/callback", get(complete_login))
        .route("/api/v2/health", get(health))
        .route("/api/v2/session", get(session))
        .route("/api/v2/notes", get(list_notes).post(create_note))
        .route(
            "/api/v2/notes/{note_id}",
            get(read_note).put(update_note).delete(delete_note),
        )
        .route("/api/v2/notes/{note_id}/source", get(export_note))
        .with_state(state)
}

async fn openapi() -> Response {
    (
        [(header::CONTENT_TYPE, "application/json; charset=utf-8")],
        OPENAPI_DOCUMENT,
    )
        .into_response()
}

#[derive(Deserialize)]
struct OidcCallbackQuery {
    code: String,
    state: String,
}

async fn begin_login(State(state): State<V3ApiState>) -> V3Result<Redirect> {
    Ok(Redirect::temporary(
        &state
            .oidc
            .begin_login()
            .await
            .map_err(authentication_error)?,
    ))
}

async fn complete_login(
    State(state): State<V3ApiState>,
    axum::extract::Query(query): axum::extract::Query<OidcCallbackQuery>,
) -> V3Result<Response> {
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
    let mut response = Redirect::to("/").into_response();
    for value in [
        format!(
            "{SESSION_COOKIE}={}; Path={}; Secure; HttpOnly; SameSite=Lax",
            session.session_id, state.cookie_path
        ),
        format!(
            "{CSRF_COOKIE}={}; Path={}; Secure; SameSite=Lax",
            session.csrf_token, state.cookie_path
        ),
    ] {
        response.headers_mut().append(
            header::SET_COOKIE,
            value.parse().map_err(|_| {
                problem(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "unavailable",
                    "authentication is unavailable",
                )
            })?,
        );
    }
    Ok(response)
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({"status": "ok", "api_version": API_VERSION}))
}

async fn session(
    State(state): State<V3ApiState>,
    headers: HeaderMap,
) -> V3Result<Json<SessionResponse>> {
    let actor = authenticated_actor(&headers, &state).await?;
    Ok(Json(SessionResponse {
        issuer: actor.issuer,
        subject: actor.subject,
        is_administrator: actor.is_administrator,
    }))
}

async fn list_notes(
    State(state): State<V3ApiState>,
    headers: HeaderMap,
) -> V3Result<Json<Vec<NoteResponse>>> {
    let actor = authenticated_actor(&headers, &state).await?;
    let notes = state
        .notes
        .list_visible_notes(actor)
        .await
        .map_err(note_error)?;
    Ok(Json(notes.into_iter().map(NoteResponse::from).collect()))
}

async fn read_note(
    State(state): State<V3ApiState>,
    Path(note_id): Path<String>,
    headers: HeaderMap,
) -> V3Result<Json<NoteResponse>> {
    let actor = authenticated_actor(&headers, &state).await?;
    let note = state
        .notes
        .read_note(actor, parse_note_id(&note_id)?)
        .await
        .map_err(note_error)?;
    Ok(Json(note.into()))
}

async fn create_note(
    State(state): State<V3ApiState>,
    headers: HeaderMap,
    Json(input): Json<NoteInput>,
) -> V3Result<(StatusCode, Json<NoteResponse>)> {
    let actor = authenticated_mutation_actor(&headers, &state).await?;
    let note = state
        .notes
        .create_note(
            actor,
            CanonicalNoteDraft {
                title: input.title,
                body: input.body,
                tags: input.tags,
            },
        )
        .await
        .map_err(note_error)?;
    Ok((StatusCode::CREATED, Json(note.into())))
}

async fn update_note(
    State(state): State<V3ApiState>,
    Path(note_id): Path<String>,
    headers: HeaderMap,
    Json(input): Json<NoteUpdateInput>,
) -> V3Result<Json<NoteResponse>> {
    let actor = authenticated_mutation_actor(&headers, &state).await?;
    let note = state
        .notes
        .update_note(
            actor,
            parse_note_id(&note_id)?,
            CanonicalNoteDraft {
                title: input.title,
                body: input.body,
                tags: input.tags,
            },
            input.expected_revision,
        )
        .await
        .map_err(note_error)?;
    Ok(Json(note.into()))
}

async fn delete_note(
    State(state): State<V3ApiState>,
    Path(note_id): Path<String>,
    headers: HeaderMap,
    Json(input): Json<DeleteInput>,
) -> V3Result<Json<NoteResponse>> {
    let actor = authenticated_mutation_actor(&headers, &state).await?;
    let note = state
        .notes
        .soft_delete_note(actor, parse_note_id(&note_id)?, input.expected_revision)
        .await
        .map_err(note_error)?;
    Ok(Json(note.into()))
}

async fn export_note(
    State(state): State<V3ApiState>,
    Path(note_id): Path<String>,
    headers: HeaderMap,
) -> V3Result<Response> {
    let actor = authenticated_actor(&headers, &state).await?;
    let note = state
        .notes
        .read_note(actor, parse_note_id(&note_id)?)
        .await
        .map_err(note_error)?;
    let source = marginalis_asciidoc::export_canonical_note(&note).map_err(|_| {
        problem(
            StatusCode::SERVICE_UNAVAILABLE,
            "unavailable",
            "note export is unavailable",
        )
    })?;
    Ok((
        [(header::CONTENT_TYPE, "text/asciidoc; charset=utf-8")],
        source,
    )
        .into_response())
}

async fn home(State(state): State<V3ApiState>, headers: HeaderMap) -> V3Result<Html<String>> {
    let actor = authenticated_actor(&headers, &state).await?;
    let notes = state
        .notes
        .list_visible_notes(actor)
        .await
        .map_err(note_error)?;
    let list = notes
        .into_iter()
        .map(|note| {
            format!(
                "<li><a href=\"/api/v2/notes/{}\">{}</a></li>",
                note.note_id,
                escape_html(&note.title)
            )
        })
        .collect::<String>();
    Ok(Html(format!(
        "<!doctype html><meta charset=\"utf-8\"><title>Marginalis</title><main><h1>Marginalis</h1><p>閲覧できるノート</p><ul>{list}</ul></main>"
    )))
}

async fn authenticated_actor(headers: &HeaderMap, state: &V3ApiState) -> V3Result<CanonicalActor> {
    let session_id = cookie_value(headers, SESSION_COOKIE).ok_or_else(|| {
        problem(
            StatusCode::UNAUTHORIZED,
            "authentication_required",
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
                "authentication_required",
                "authentication is required",
            )
        })
}

async fn authenticated_mutation_actor(
    headers: &HeaderMap,
    state: &V3ApiState,
) -> V3Result<CanonicalActor> {
    let actor = authenticated_actor(headers, state).await?;
    if headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
        != Some(state.browser_origin.as_str())
    {
        return Err(problem(
            StatusCode::FORBIDDEN,
            "same_origin_required",
            "same-origin request is required",
        ));
    }
    if let Some(site) = headers
        .get("sec-fetch-site")
        .and_then(|value| value.to_str().ok())
    {
        if !matches!(site, "same-origin" | "none") {
            return Err(problem(
                StatusCode::FORBIDDEN,
                "same_origin_required",
                "same-origin request is required",
            ));
        }
    }
    let session_id =
        cookie_value(headers, SESSION_COOKIE).expect("authenticated session cookie exists");
    let csrf_cookie = cookie_value(headers, CSRF_COOKIE).ok_or_else(|| {
        problem(
            StatusCode::FORBIDDEN,
            "csrf_required",
            "CSRF token is required",
        )
    })?;
    let csrf_header = headers
        .get("x-csrf-token")
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| {
            problem(
                StatusCode::FORBIDDEN,
                "csrf_required",
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
            "csrf_invalid",
            "CSRF token is invalid",
        ));
    }
    Ok(actor)
}

fn parse_note_id(value: &str) -> V3Result<NoteId> {
    EntityId::from_str(value)
        .map(NoteId::new)
        .map_err(|_| problem(StatusCode::NOT_FOUND, "not_found", "note is not available"))
}

fn cookie_value(headers: &HeaderMap, name: &str) -> Option<String> {
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

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use axum::{body::Body, http::Request};
    use marginalis_application::{AuthenticationUseCaseError, NoteUseCaseError};
    use marginalis_domain::{CanonicalAuthenticatedSession, CanonicalWebSession};
    use tower::ServiceExt;

    struct Notes;

    #[async_trait]
    impl V3NoteUseCases for Notes {
        async fn list_visible_notes(
            &self,
            _actor: CanonicalActor,
        ) -> Result<Vec<CanonicalNote>, NoteUseCaseError> {
            Ok(Vec::new())
        }

        async fn read_note(
            &self,
            _actor: CanonicalActor,
            _note_id: NoteId,
        ) -> Result<CanonicalNote, NoteUseCaseError> {
            Err(NoteUseCaseError::NotFound)
        }

        async fn create_note(
            &self,
            _actor: CanonicalActor,
            _draft: CanonicalNoteDraft,
        ) -> Result<CanonicalNote, NoteUseCaseError> {
            Err(NoteUseCaseError::Unavailable)
        }

        async fn update_note(
            &self,
            _actor: CanonicalActor,
            _note_id: NoteId,
            _draft: CanonicalNoteDraft,
            _expected_revision: i64,
        ) -> Result<CanonicalNote, NoteUseCaseError> {
            Err(NoteUseCaseError::Unavailable)
        }

        async fn soft_delete_note(
            &self,
            _actor: CanonicalActor,
            _note_id: NoteId,
            _expected_revision: i64,
        ) -> Result<CanonicalNote, NoteUseCaseError> {
            Err(NoteUseCaseError::Unavailable)
        }
    }

    struct Sessions;

    #[async_trait]
    impl V3WebSessionUseCases for Sessions {
        async fn authenticate_session(
            &self,
            _session_id: String,
        ) -> Result<Option<CanonicalAuthenticatedSession>, AuthenticationUseCaseError> {
            Ok(None)
        }

        async fn verify_csrf(
            &self,
            _session_id: String,
            _csrf_token: String,
        ) -> Result<bool, AuthenticationUseCaseError> {
            Ok(false)
        }

        async fn issue_session(
            &self,
            _actor: CanonicalActor,
        ) -> Result<CanonicalWebSession, AuthenticationUseCaseError> {
            Err(AuthenticationUseCaseError::Unavailable)
        }
    }

    struct Oidc;

    #[async_trait]
    impl V3OidcAuthenticationUseCases for Oidc {
        async fn begin_login(&self) -> Result<String, AuthenticationUseCaseError> {
            Err(AuthenticationUseCaseError::Unavailable)
        }

        async fn complete_login(
            &self,
            _code: String,
            _state: String,
        ) -> Result<CanonicalActor, AuthenticationUseCaseError> {
            Err(AuthenticationUseCaseError::Unavailable)
        }
    }

    fn app() -> Router {
        router(V3ApiState::new(
            Arc::new(Notes),
            Arc::new(Sessions),
            Arc::new(Oidc),
            "/".into(),
            "https://example.test".into(),
        ))
    }

    #[tokio::test]
    async fn v3_health_is_public_but_notes_require_a_v3_session() {
        let health = app()
            .oneshot(
                Request::get("/api/v2/health")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(health.status(), StatusCode::OK);
        let notes = app()
            .oneshot(
                Request::get("/api/v2/notes")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(notes.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn v3_openapi_is_served_from_the_embedded_contract() {
        let response = app()
            .oneshot(
                Request::get("/api/v2/openapi.json")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            OPENAPI_DOCUMENT,
            include_str!("../../../docs/openapi-v3.json")
        );
    }
}
