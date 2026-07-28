//! REST note APIとsession introspection。

use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use marginalis_application::NoteRenderContext;
use marginalis_domain::{Note, NoteAclEntry, NoteDraft};
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

#[derive(Serialize)]
pub(super) struct NotePreviewResponse {
    html: String,
}

impl From<Note> for NoteResponse {
    fn from(note: Note) -> Self {
        Self {
            note_id: note.note_id().to_string(),
            title: note.title().to_owned(),
            body: note.body().to_owned(),
            tags: note.tags().to_vec(),
            created_at_ms: note.created_at().get(),
            updated_at_ms: note.updated_at().get(),
            revision: note.revision(),
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

#[derive(Serialize)]
pub(super) struct NoteAclResponse {
    entries: Vec<NoteAclEntry>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct NoteAclInput {
    entries: Vec<NoteAclEntry>,
    expected_revision: i64,
}

pub(super) async fn session(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> HandlerResult<Json<SessionResponse>> {
    let actor = authenticated_actor(&headers, &state).await?;
    Ok(Json(SessionResponse {
        issuer: actor.issuer().to_owned(),
        subject: actor.subject().to_owned(),
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

pub(super) async fn preview_note(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(input): Json<NoteInput>,
) -> HandlerResult<Json<NotePreviewResponse>> {
    let actor = authenticated_mutation_actor(&headers, &state).await?;
    let html = state
        .notes
        .preview_note(
            actor,
            NoteDraft {
                title: input.title,
                body: input.body,
                tags: input.tags,
            },
            NoteRenderContext {
                note_path_prefix: super::auth::external_path(&state.cookie_path, "/notes"),
            },
        )
        .await
        .map_err(note_error)?;
    Ok(Json(NotePreviewResponse { html }))
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

pub(super) async fn read_note_acl(
    State(state): State<ApiState>,
    Path(note_id): Path<String>,
    headers: HeaderMap,
) -> HandlerResult<Json<NoteAclResponse>> {
    let actor = authenticated_actor(&headers, &state).await?;
    let entries = state
        .notes
        .read_note_acl(actor, parse_note_id(&note_id)?)
        .await
        .map_err(note_error)?;
    Ok(Json(NoteAclResponse { entries }))
}

pub(super) async fn replace_note_acl(
    State(state): State<ApiState>,
    Path(note_id): Path<String>,
    headers: HeaderMap,
    Json(input): Json<NoteAclInput>,
) -> HandlerResult<Json<NoteResponse>> {
    let actor = authenticated_mutation_actor(&headers, &state).await?;
    let note = state
        .notes
        .replace_note_acl(
            actor,
            parse_note_id(&note_id)?,
            input.entries,
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
