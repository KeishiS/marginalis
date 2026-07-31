//! REST note APIとsession introspection。

use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use marginalis_application::{
    NoteAclChange, NoteGraphQuery, NoteRenderContext, NoteView, NoteWritePolicy,
};
use marginalis_contract::{
    NoteAclGrantResponse, NoteAclResponse, NoteAclUpdateInput, NoteDraftInput,
    NoteGraphCitationResponse, NoteGraphNoteResponse, NoteGraphReferenceResponse,
    NoteGraphResponse, NoteGraphWorkResponse, NoteListEntryResponse, NotePreviewResponse,
    NoteResponse, NoteSummaryResponse, NoteViewResponse, ProblemCode, RelatedNotesResponse,
    SessionResponse,
};
use marginalis_domain::{Note, NoteDraft, NoteSummary, Revision};
use serde::Deserialize;

use super::{
    auth::{authenticated_actor, authenticated_mutation_actor, parse_note_id},
    error::{HandlerResult, note_error, problem},
    state::ApiState,
};

pub(super) type NoteInput = NoteDraftInput;

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
) -> HandlerResult<Json<Vec<NoteListEntryResponse>>> {
    let actor = authenticated_actor(&headers, &state).await?;
    let notes = state
        .notes
        .list_visible_notes(actor)
        .await
        .map_err(note_error)?;
    Ok(Json(
        notes
            .into_iter()
            .map(|entry| NoteListEntryResponse {
                note_id: entry.summary.note_id.to_string(),
                title: entry.summary.title,
                tags: entry.summary.tags,
                updated_at_ms: entry.summary.updated_at.get(),
                revision: entry.summary.revision.get(),
                access: entry.access,
            })
            .collect(),
    ))
}

pub(super) async fn read_note(
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
    Ok(note_json(StatusCode::OK, note))
}

pub(super) async fn read_note_view(
    State(state): State<ApiState>,
    Path(note_id): Path<String>,
    headers: HeaderMap,
) -> HandlerResult<Response> {
    let actor = authenticated_actor(&headers, &state).await?;
    let view = state
        .notes
        .read_note_view(
            actor,
            parse_note_id(&note_id)?,
            NoteRenderContext {
                note_path_prefix: super::auth::external_path(&state.cookie_path, "/notes"),
            },
        )
        .await
        .map_err(note_error)?;
    let revision = view.note.revision();
    Ok((
        [(header::ETAG, etag(revision))],
        Json(note_view_response(view)),
    )
        .into_response())
}

pub(super) async fn create_note(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(input): Json<NoteInput>,
) -> HandlerResult<Response> {
    let actor = authenticated_mutation_actor(&headers, &state).await?;
    let note = state
        .notes
        .create_note(
            actor,
            NoteDraft {
                source: input.source,
                title: String::new(),
                tags: Vec::new(),
            },
            NoteWritePolicy::AllowAdvisories,
        )
        .await
        .map_err(note_error)?;
    Ok(note_json(StatusCode::CREATED, note))
}

pub(super) async fn preview_note(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(input): Json<NoteInput>,
) -> HandlerResult<Json<NotePreviewResponse>> {
    let actor = authenticated_mutation_actor(&headers, &state).await?;
    let preview = state
        .notes
        .preview_note(
            actor,
            NoteDraft {
                source: input.source,
                title: String::new(),
                tags: Vec::new(),
            },
            NoteRenderContext {
                note_path_prefix: super::auth::external_path(&state.cookie_path, "/notes"),
            },
        )
        .await
        .map_err(note_error)?;
    tracing::Span::current().record("note_diagnostic_count", preview.diagnostics.len());
    Ok(Json(NotePreviewResponse {
        html: preview.html,
        diagnostics: preview
            .diagnostics
            .into_iter()
            .map(super::error::advisory_response)
            .collect(),
    }))
}

pub(super) async fn update_note(
    State(state): State<ApiState>,
    Path(note_id): Path<String>,
    headers: HeaderMap,
    Json(input): Json<NoteDraftInput>,
) -> HandlerResult<Response> {
    let actor = authenticated_mutation_actor(&headers, &state).await?;
    let note = state
        .notes
        .update_note(
            actor,
            parse_note_id(&note_id)?,
            NoteDraft {
                source: input.source,
                title: String::new(),
                tags: Vec::new(),
            },
            expected_revision(&headers)?,
            NoteWritePolicy::AllowAdvisories,
        )
        .await
        .map_err(note_error)?;
    Ok(note_json(StatusCode::OK, note))
}

pub(super) async fn read_note_acl(
    State(state): State<ApiState>,
    Path(note_id): Path<String>,
    headers: HeaderMap,
) -> HandlerResult<Response> {
    let actor = authenticated_actor(&headers, &state).await?;
    let acl = state
        .notes
        .read_note_acl(actor, parse_note_id(&note_id)?)
        .await
        .map_err(note_error)?;
    let response = NoteAclResponse {
        entries: acl
            .entries
            .into_iter()
            .map(|entry| NoteAclGrantResponse {
                issuer: entry.identity().issuer().to_owned(),
                subject: entry.identity().subject().to_owned(),
                permission: entry.permission(),
            })
            .collect(),
    };
    Ok(([(header::ETAG, etag(acl.revision))], Json(response)).into_response())
}

pub(super) async fn replace_note_acl(
    State(state): State<ApiState>,
    Path(note_id): Path<String>,
    headers: HeaderMap,
    Json(input): Json<NoteAclUpdateInput>,
) -> HandlerResult<Response> {
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
                    permission: entry.permission,
                })
                .collect(),
            expected_revision(&headers)?,
        )
        .await
        .map_err(note_error)?;
    Ok(note_json(StatusCode::OK, note))
}

pub(super) async fn delete_note(
    State(state): State<ApiState>,
    Path(note_id): Path<String>,
    headers: HeaderMap,
) -> HandlerResult<Response> {
    let actor = authenticated_mutation_actor(&headers, &state).await?;
    let note = state
        .notes
        .soft_delete_note(
            actor,
            parse_note_id(&note_id)?,
            expected_revision(&headers)?,
        )
        .await
        .map_err(note_error)?;
    Ok(note_json(StatusCode::OK, note))
}

pub(super) async fn restore_note(
    State(state): State<ApiState>,
    Path(note_id): Path<String>,
    headers: HeaderMap,
) -> HandlerResult<Response> {
    let actor = authenticated_mutation_actor(&headers, &state).await?;
    let note = state
        .notes
        .restore_note(
            actor,
            parse_note_id(&note_id)?,
            expected_revision(&headers)?,
        )
        .await
        .map_err(note_error)?;
    Ok(note_json(StatusCode::OK, note))
}

pub(super) fn expected_revision(headers: &HeaderMap) -> HandlerResult<Revision> {
    let value = headers.get(header::IF_MATCH).ok_or_else(|| {
        problem(
            StatusCode::PRECONDITION_REQUIRED,
            ProblemCode::PreconditionRequired,
            "If-Match is required",
        )
    })?;
    let value = value.to_str().ok().and_then(|value| {
        value
            .strip_prefix("\"rev-")
            .and_then(|value| value.strip_suffix('"'))
            .and_then(|value| value.parse::<i64>().ok())
    });
    value
        .and_then(|value| Revision::new(value).ok())
        .ok_or_else(|| {
            problem(
                StatusCode::BAD_REQUEST,
                ProblemCode::InvalidRequest,
                "If-Match must contain one strong note revision",
            )
        })
}

pub(super) fn etag(revision: Revision) -> HeaderValue {
    HeaderValue::from_str(&format!("\"rev-{}\"", revision.get())).expect("valid ETag")
}

fn note_json(status: StatusCode, note: Note) -> Response {
    let revision = note.revision();
    (
        status,
        [(header::ETAG, etag(revision))],
        Json(note_response(note)),
    )
        .into_response()
}

fn note_response(note: Note) -> NoteResponse {
    NoteResponse {
        note_id: note.note_id().to_string(),
        title: note.title().to_owned(),
        source: note.source().to_owned(),
        tags: note.tags().to_vec(),
        created_at_ms: note.created_at().get(),
        updated_at_ms: note.updated_at().get(),
        revision: note.revision().get(),
    }
}

fn note_summary_response(note: NoteSummary) -> NoteSummaryResponse {
    NoteSummaryResponse {
        note_id: note.note_id.to_string(),
        title: note.title,
        tags: note.tags,
        updated_at_ms: note.updated_at.get(),
        revision: note.revision.get(),
    }
}

fn note_view_response(view: NoteView) -> NoteViewResponse {
    NoteViewResponse {
        note: note_response(view.note),
        access: view.access,
        html: view.html,
        related: RelatedNotesResponse {
            outgoing: view
                .related
                .outgoing
                .into_iter()
                .map(note_summary_response)
                .collect(),
            incoming: view
                .related
                .incoming
                .into_iter()
                .map(note_summary_response)
                .collect(),
        },
    }
}

#[derive(Default, Deserialize)]
pub(super) struct NoteGraphParameters {
    #[serde(default)]
    query: String,
}

/// 閲覧できるノートと、それらが引用する文献の関係を返す。
///
/// 認可はSQLite問い合わせの中で適用済みであり、ここでは形を写すだけにする。ここで絞り込むと、
/// 絞り込み漏れがそのまま情報の開示になる。
pub(super) async fn read_note_graph(
    State(state): State<ApiState>,
    Query(parameters): Query<NoteGraphParameters>,
    headers: HeaderMap,
) -> HandlerResult<Json<NoteGraphResponse>> {
    let actor = authenticated_actor(&headers, &state).await?;
    let graph = state
        .notes
        .read_note_graph(
            actor,
            NoteGraphQuery {
                text: Some(parameters.query),
            },
        )
        .await
        .map_err(note_error)?;
    Ok(Json(NoteGraphResponse {
        notes: graph
            .notes
            .into_iter()
            .map(|note| NoteGraphNoteResponse {
                note_id: note.note_id.to_string(),
                title: note.title,
                tags: note.tags,
                updated_at_ms: note.updated_at.get(),
            })
            .collect(),
        works: graph
            .works
            .into_iter()
            .map(|work| NoteGraphWorkResponse {
                citation_key: work.citation_key,
                title: work.title,
            })
            .collect(),
        references: graph
            .references
            .into_iter()
            .map(|edge| NoteGraphReferenceResponse {
                source_note_id: edge.source_note_id.to_string(),
                target_note_id: edge.target_note_id.to_string(),
            })
            .collect(),
        citations: graph
            .citations
            .into_iter()
            .map(|edge| NoteGraphCitationResponse {
                source_note_id: edge.source_note_id.to_string(),
                citation_key: edge.citation_key,
            })
            .collect(),
    }))
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
            ProblemCode::Unavailable,
            "note export is unavailable",
        )
    })?;
    Ok((
        [(header::CONTENT_TYPE, "text/asciidoc; charset=utf-8")],
        source,
    )
        .into_response())
}
