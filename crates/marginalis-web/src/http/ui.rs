//! 最小の閲覧用HTML UI。

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse, Response},
};
use marginalis_application::{NoteRenderContext, RelatedNotes};
use marginalis_domain::{Note, UnixMillis};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use super::{
    auth::{authenticated_ui_actor, external_path, parse_note_id},
    error::{HandlerResult, note_error, problem},
    html::{escape_html, page_document, page_document_with_script},
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
        format!(
            "<h1>ノート</h1><p><a href=\"{}\">新規ノート</a></p><p>閲覧できるノートはありません。</p>",
            external_path(&state.cookie_path, "/notes/new")
        )
    } else {
        format!(
            "<h1>ノート</h1><p><a href=\"{}\">新規ノート</a></p><p>閲覧できるノート</p><ul>{list}</ul>",
            external_path(&state.cookie_path, "/notes/new")
        )
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
        .read_note(actor.clone(), parse_note_id(&note_id)?)
        .await
        .map_err(note_error)?;
    let body = state
        .notes
        .render_note_html(
            actor.clone(),
            note.note_id,
            NoteRenderContext {
                note_path_prefix: external_path(&state.cookie_path, "/notes"),
            },
        )
        .await
        .map_err(|_| {
            problem(
                StatusCode::UNPROCESSABLE_ENTITY,
                "render_failed",
                "note cannot be rendered safely",
            )
        })?;
    let related = state
        .notes
        .related_notes(actor, note.note_id)
        .await
        .map_err(note_error)?;
    let content = format!(
        "<nav aria-label=\"ノート操作\"><a href=\"{}\">一覧</a> <a href=\"{}\">編集</a></nav>{}{}",
        external_path(&state.cookie_path, "/"),
        external_path(&state.cookie_path, &format!("/notes/{}/edit", note.note_id)),
        body,
        related_notes_html(&state.cookie_path, related)
    );
    Ok(Html(page_document(&note.title, &state.cookie_path, &content)).into_response())
}

fn related_notes_html(cookie_path: &str, related: RelatedNotes) -> String {
    format!(
        "<div class=\"related-notes\">{}{}</div>",
        related_note_list(
            cookie_path,
            "outgoing-notes",
            "このノートが参照しているノート",
            "参照しているノートはありません。",
            &related.outgoing,
        ),
        related_note_list(
            cookie_path,
            "incoming-notes",
            "このノートを参照しているノート",
            "このノートを参照しているノートはありません。",
            &related.incoming,
        )
    )
}

fn related_note_list(
    cookie_path: &str,
    heading_id: &str,
    heading: &str,
    empty_message: &str,
    notes: &[Note],
) -> String {
    let heading = format!("<h2 id=\"{heading_id}\">{}</h2>", escape_html(heading));
    if notes.is_empty() {
        return format!(
            "<section aria-labelledby=\"{heading_id}\">{heading}<p>{}</p></section>",
            escape_html(empty_message)
        );
    }

    let initial = notes
        .iter()
        .take(10)
        .map(|note| related_note_item(cookie_path, note))
        .collect::<String>();
    let remaining = notes
        .iter()
        .skip(10)
        .map(|note| related_note_item(cookie_path, note))
        .collect::<String>();
    let more = if remaining.is_empty() {
        String::new()
    } else {
        format!("<details><summary>さらに表示</summary><ul>{remaining}</ul></details>")
    };
    format!("<section aria-labelledby=\"{heading_id}\">{heading}<ul>{initial}</ul>{more}</section>")
}

fn related_note_item(cookie_path: &str, note: &Note) -> String {
    let mut tags = note.tags.clone();
    tags.sort();
    let visible_tags = tags
        .iter()
        .take(2)
        .map(|tag| format!("<li>{}</li>", escape_html(tag)))
        .collect::<String>();
    let hidden_tags = tags
        .iter()
        .skip(2)
        .map(|tag| format!("<li>{}</li>", escape_html(tag)))
        .collect::<String>();
    let more_tags = if hidden_tags.is_empty() {
        String::new()
    } else {
        format!(
            "<details><summary>+{}</summary><ul>{hidden_tags}</ul></details>",
            tags.len() - 2
        )
    };
    let updated_at = format_unix_millis(note.updated_at);
    format!(
        "<li><a href=\"{}\">{}</a><ul aria-label=\"タグ\">{visible_tags}</ul>{more_tags}<p>更新日時: <time datetime=\"{}\">{}</time></p></li>",
        escape_html(&external_path(
            cookie_path,
            &format!("/notes/{}", note.note_id)
        )),
        escape_html(&note.title),
        escape_html(&updated_at),
        escape_html(&updated_at)
    )
}

fn format_unix_millis(value: UnixMillis) -> String {
    OffsetDateTime::from_unix_timestamp_nanos(i128::from(value.get()) * 1_000_000)
        .ok()
        .and_then(|value| value.format(&Rfc3339).ok())
        .unwrap_or_else(|| value.get().to_string())
}

pub(super) async fn create_note_page(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> HandlerResult<Response> {
    let return_to = external_path(&state.cookie_path, "/notes/new");
    if let Err(response) = authenticated_ui_actor(&headers, &state, &return_to).await {
        return Ok(response);
    }
    Ok(editor_page(&state, None))
}

pub(super) async fn edit_note_page(
    State(state): State<ApiState>,
    Path(note_id): Path<String>,
    headers: HeaderMap,
) -> HandlerResult<Response> {
    let return_to = external_path(&state.cookie_path, &format!("/notes/{note_id}/edit"));
    let actor = match authenticated_ui_actor(&headers, &state, &return_to).await {
        Ok(actor) => actor,
        Err(response) => return Ok(response),
    };
    let note_id = parse_note_id(&note_id)?;
    state
        .notes
        .read_note(actor, note_id)
        .await
        .map_err(note_error)?;
    Ok(editor_page(&state, Some(note_id)))
}

fn editor_page(state: &ApiState, note_id: Option<marginalis_domain::NoteId>) -> Response {
    let mode = if note_id.is_some() { "edit" } else { "create" };
    let note_id = note_id.map_or_else(String::new, |note_id| note_id.to_string());
    let api_base = external_path(&state.cookie_path, "/api/v2");
    let content = format!(
        "<div data-editor-application data-mode=\"{mode}\" data-note-id=\"{}\" data-api-base=\"{}\" data-base-path=\"{}\"><p>編集画面を読み込んでいます。</p></div><noscript>ノートの編集にはJavaScriptが必要です。</noscript>",
        escape_html(&note_id),
        escape_html(&api_base),
        escape_html(&state.cookie_path)
    );
    Html(page_document_with_script(
        "ノートの編集",
        &state.cookie_path,
        &content,
        Some("/assets/editor.js"),
    ))
    .into_response()
}
