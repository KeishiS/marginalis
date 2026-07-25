//! 最小の閲覧用HTML UI。

use super::*;

pub(super) async fn home(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> HandlerResult<Html<String>> {
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
                "<li><a href=\"{}\">{}</a></li>",
                external_path(&state.cookie_path, &format!("/notes/{}", note.note_id)),
                escape_html(&note.title)
            )
        })
        .collect::<String>();
    Ok(Html(format!(
        "<!doctype html><meta charset=\"utf-8\"><title>Marginalis</title><main><h1>Marginalis</h1><p>閲覧できるノート</p><ul>{list}</ul></main>"
    )))
}

pub(super) async fn view_note(
    State(state): State<ApiState>,
    Path(note_id): Path<String>,
    headers: HeaderMap,
) -> HandlerResult<Html<String>> {
    let actor = authenticated_actor(&headers, &state).await?;
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
    Ok(Html(format!(
        "<!doctype html><meta charset=\"utf-8\"><title>{}</title><main><p><a href=\"{}\">一覧</a></p>{}</main>",
        escape_html(&note.title),
        external_path(&state.cookie_path, "/"),
        body
    )))
}
