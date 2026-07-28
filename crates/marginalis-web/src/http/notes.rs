//! REST note APIとsession introspection。

use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use marginalis_application::{NoteAclChange, NoteRenderContext};
use marginalis_domain::{Note, NoteAclEntry, NoteDraft, NotePermission, NoteSummary, Revision};
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
pub(super) struct NoteSummaryResponse {
    note_id: String,
    title: String,
    tags: Vec<String>,
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
            revision: note.revision().get(),
        }
    }
}

impl From<NoteSummary> for NoteSummaryResponse {
    fn from(note: NoteSummary) -> Self {
        Self {
            note_id: note.note_id.to_string(),
            title: note.title,
            tags: note.tags,
            updated_at_ms: note.updated_at.get(),
            revision: note.revision.get(),
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
    entries: Vec<NoteAclEntryResponse>,
}

#[derive(Serialize)]
pub(super) struct NoteAclEntryResponse {
    issuer: String,
    subject: String,
    permission: RestNotePermission,
}

impl From<NoteAclEntry> for NoteAclEntryResponse {
    fn from(entry: NoteAclEntry) -> Self {
        Self {
            issuer: entry.identity().issuer().to_owned(),
            subject: entry.identity().subject().to_owned(),
            permission: RestNotePermission::from(entry.permission()),
        }
    }
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum RestNotePermission {
    Read,
    Edit,
}

impl From<NotePermission> for RestNotePermission {
    fn from(permission: NotePermission) -> Self {
        match permission {
            NotePermission::Read => Self::Read,
            NotePermission::Edit => Self::Edit,
        }
    }
}

impl From<RestNotePermission> for NotePermission {
    fn from(permission: RestNotePermission) -> Self {
        match permission {
            RestNotePermission::Read => Self::Read,
            RestNotePermission::Edit => Self::Edit,
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct NoteAclEntryInput {
    subject: String,
    permission: RestNotePermission,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct NoteAclInput {
    entries: Vec<NoteAclEntryInput>,
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
) -> HandlerResult<Json<Vec<NoteSummaryResponse>>> {
    let actor = authenticated_actor(&headers, &state).await?;
    let notes = state
        .notes
        .list_visible_notes(actor)
        .await
        .map_err(note_error)?;
    Ok(Json(
        notes.into_iter().map(NoteSummaryResponse::from).collect(),
    ))
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
            revision(input.expected_revision)?,
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
    Ok(Json(NoteAclResponse {
        entries: entries
            .into_iter()
            .map(NoteAclEntryResponse::from)
            .collect(),
    }))
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
            input
                .entries
                .into_iter()
                .map(|entry| NoteAclChange {
                    subject: entry.subject,
                    permission: entry.permission.into(),
                })
                .collect(),
            revision(input.expected_revision)?,
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
        .soft_delete_note(
            actor,
            parse_note_id(&note_id)?,
            revision(input.expected_revision)?,
        )
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
        .restore_note(
            actor,
            parse_note_id(&note_id)?,
            revision(input.expected_revision)?,
        )
        .await
        .map_err(note_error)?;
    Ok(Json(note.into()))
}

fn revision(value: i64) -> HandlerResult<Revision> {
    Revision::new(value).map_err(|_| {
        problem(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "expected_revision must be positive",
        )
    })
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
