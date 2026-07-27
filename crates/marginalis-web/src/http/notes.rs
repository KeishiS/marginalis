//! REST note APIとsession introspection。

use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use marginalis_domain::{Note, NoteDraft};
use serde::{Deserialize, Serialize};

use super::{
    auth::{authenticated_actor, authenticated_mutation_actor, parse_note_id},
    error::{HandlerResult, note_error, problem},
    state::ApiState,
};

#[derive(Serialize)]
pub(super) struct SessionResponse {
    issuer: String,
    subject: String,
    is_administrator: bool,
}

#[derive(Serialize)]
pub(super) struct NoteResponse {
    note_id: String,
    title: String,
    body: String,
    tags: Vec<String>,
    created_at_ms: i64,
    updated_at_ms: i64,
    revision: i64,
}

impl From<Note> for NoteResponse {
    fn from(note: Note) -> Self {
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
#[serde(deny_unknown_fields)]
pub(super) struct NoteInput {
    pub(super) title: String,
    pub(super) body: String,
    pub(super) tags: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct NoteUpdateInput {
    title: String,
    body: String,
    tags: Vec<String>,
    expected_revision: i64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct DeleteInput {
    expected_revision: i64,
}

pub(super) async fn session(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> HandlerResult<Json<SessionResponse>> {
    let actor = authenticated_actor(&headers, &state).await?;
    Ok(Json(SessionResponse {
        issuer: actor.issuer,
        subject: actor.subject,
        is_administrator: actor.is_administrator,
    }))
}

pub(super) async fn list_notes(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> HandlerResult<Json<Vec<NoteResponse>>> {
    let actor = authenticated_actor(&headers, &state).await?;
    let notes = state
        .notes
        .list_visible_notes(actor)
        .await
        .map_err(note_error)?;
    Ok(Json(notes.into_iter().map(NoteResponse::from).collect()))
}

pub(super) async fn read_note(
    State(state): State<ApiState>,
    Path(note_id): Path<String>,
    headers: HeaderMap,
) -> HandlerResult<Json<NoteResponse>> {
    let actor = authenticated_actor(&headers, &state).await?;
    let note = state
        .notes
        .read_note(actor, parse_note_id(&note_id)?)
        .await
        .map_err(note_error)?;
    Ok(Json(note.into()))
}

pub(super) async fn create_note(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(input): Json<NoteInput>,
) -> HandlerResult<(StatusCode, Json<NoteResponse>)> {
    let actor = authenticated_mutation_actor(&headers, &state).await?;
    let note = state
        .notes
        .create_note(
            actor,
            NoteDraft {
                title: input.title,
                body: input.body,
                tags: input.tags,
            },
        )
        .await
        .map_err(note_error)?;
    Ok((StatusCode::CREATED, Json(note.into())))
}

pub(super) async fn update_note(
    State(state): State<ApiState>,
    Path(note_id): Path<String>,
    headers: HeaderMap,
    Json(input): Json<NoteUpdateInput>,
) -> HandlerResult<Json<NoteResponse>> {
    let actor = authenticated_mutation_actor(&headers, &state).await?;
    let note = state
        .notes
        .update_note(
            actor,
            parse_note_id(&note_id)?,
            NoteDraft {
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

pub(super) async fn delete_note(
    State(state): State<ApiState>,
    Path(note_id): Path<String>,
    headers: HeaderMap,
    Json(input): Json<DeleteInput>,
) -> HandlerResult<Json<NoteResponse>> {
    let actor = authenticated_mutation_actor(&headers, &state).await?;
    let note = state
        .notes
        .soft_delete_note(actor, parse_note_id(&note_id)?, input.expected_revision)
        .await
        .map_err(note_error)?;
    Ok(Json(note.into()))
}

pub(super) async fn restore_note(
    State(state): State<ApiState>,
    Path(note_id): Path<String>,
    headers: HeaderMap,
    Json(input): Json<DeleteInput>,
) -> HandlerResult<Json<NoteResponse>> {
    let actor = authenticated_mutation_actor(&headers, &state).await?;
    let note = state
        .notes
        .restore_note(actor, parse_note_id(&note_id)?, input.expected_revision)
        .await
        .map_err(note_error)?;
    Ok(Json(note.into()))
}

pub(super) async fn export_note(
    State(state): State<ApiState>,
    Path(note_id): Path<String>,
    headers: HeaderMap,
) -> HandlerResult<Response> {
    let actor = authenticated_actor(&headers, &state).await?;
    let note = state
        .notes
        .read_note(actor, parse_note_id(&note_id)?)
        .await
        .map_err(note_error)?;
    let source = state.notes.export_note_source(&note).map_err(|_| {
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
