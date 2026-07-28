//! 最小の閲覧用HTML UI。

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse, Response},
};

use super::{
    auth::{authenticated_ui_actor, external_path, parse_note_id},
    error::{HandlerResult, note_error, problem},
    html::{escape_html, page_document},
    state::ApiState,
};

pub(super) async fn home(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> HandlerResult<Response> {
    let return_to = external_path(&state.cookie_path, "/");
    let actor = match authenticated_ui_actor(&headers, &state, &return_to).await {
        Ok(actor) => actor,
        Err(response) => return Ok(response),
    };
    let notes = state
        .notes
        .list_visible_notes(actor)
        .await
        .map_err(note_error)?;
    let list = notes
        .into_iter()
        .map(|note| {
            format!(
                "<li><a href=\"{}\">{}</a></li>",
                external_path(&state.cookie_path, &format!("/notes/{}", note.note_id)),
                escape_html(&note.title)
            )
        })
        .collect::<String>();
    let content = if list.is_empty() {
        "<h1>ノート</h1><p>閲覧できるノートはありません。</p>".to_owned()
    } else {
        format!("<h1>ノート</h1><p>閲覧できるノート</p><ul>{list}</ul>")
    };
    Ok(Html(page_document("Marginalis", &state.cookie_path, &content)).into_response())
}

pub(super) async fn view_note(
    State(state): State<ApiState>,
    Path(note_id): Path<String>,
    headers: HeaderMap,
) -> HandlerResult<Response> {
    let return_to = external_path(&state.cookie_path, &format!("/notes/{note_id}"));
    let actor = match authenticated_ui_actor(&headers, &state, &return_to).await {
        Ok(actor) => actor,
        Err(response) => return Ok(response),
    };
    let note = state
        .notes
        .read_note(actor, parse_note_id(&note_id)?)
        .await
        .map_err(note_error)?;
    let body = state.notes.render_note_html(&note).map_err(|_| {
        problem(
            StatusCode::UNPROCESSABLE_ENTITY,
            "render_failed",
            "note cannot be rendered safely",
        )
    })?;
    let content = format!(
        "<p><a href=\"{}\">一覧</a></p>{}",
        external_path(&state.cookie_path, "/"),
        body
    );
    Ok(Html(page_document(&note.title, &state.cookie_path, &content)).into_response())
}
