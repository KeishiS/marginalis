//! 閲覧画面へ直接参照の一覧を描画する。

use marginalis_application::RelatedNotes;
use marginalis_domain::{NoteSummary, UnixMillis};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use super::{auth::external_path, html::escape_html};

pub(super) fn related_notes_html(cookie_path: &str, related: RelatedNotes) -> String {
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
    notes: &[NoteSummary],
) -> String {
    let heading_markup = format!("<h2 id=\"{heading_id}\">{}</h2>", escape_html(heading));
    if notes.is_empty() {
        return format!(
            "<section aria-labelledby=\"{heading_id}\">{heading_markup}<p>{}</p></section>",
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
        format!(
            "<details><summary>{}をさらに表示</summary><ul>{remaining}</ul></details>",
            escape_html(heading)
        )
    };
    format!(
        "<section aria-labelledby=\"{heading_id}\">{heading_markup}<ul>{initial}</ul>{more}</section>"
    )
}

fn related_note_item(cookie_path: &str, note: &NoteSummary) -> String {
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
            "<details><summary>「{}」の残りのタグ{}件</summary><ul>{hidden_tags}</ul></details>",
            escape_html(&note.title),
            tags.len() - 2
        )
    };
    let updated_at = formatted_time(note.updated_at);
    format!(
        "<li><a href=\"{}\">{}</a><ul aria-label=\"タグ\">{visible_tags}</ul>{more_tags}<p>更新日時: {updated_at}</p></li>",
        escape_html(&external_path(
            cookie_path,
            &format!("/notes/{}", note.note_id)
        )),
        escape_html(&note.title),
    )
}

fn formatted_time(value: UnixMillis) -> String {
    let Some(timestamp) =
        OffsetDateTime::from_unix_timestamp_nanos(i128::from(value.get()) * 1_000_000)
            .ok()
            .and_then(|value| value.format(&Rfc3339).ok())
    else {
        return "表示できません".into();
    };
    format!(
        "<time datetime=\"{}\" data-local-time>{}</time>",
        escape_html(&timestamp),
        escape_html(&timestamp)
    )
}
