//! 閲覧できるノートと引用文献の関係図の読み取り。

use std::collections::{BTreeMap, HashSet};

use marginalis_application::{
    NoteGraph, NoteGraphCitation, NoteGraphNote, NoteGraphQuery, NoteGraphReference, NoteGraphWork,
};
use marginalis_domain::{Actor, UnixMillis, ValidatedCslJson};
use sqlx::Row;

use crate::{SqliteDatabase, SqliteStoreError, database_error, notes::note_id_from_text};

impl SqliteDatabase {
    /// 閲覧できるノートと、それらが引用する文献の関係を1回の読み取りtransactionで返す。
    ///
    /// 認可は各問い合わせの中で`note_access`へ結合して適用する。取得後に絞り込む形にすると、
    /// 絞り込み漏れがそのまま情報の開示になる。線は始点と終点の両方が可視な場合だけ返すため、
    /// 閲覧できないノートの存在も件数も現れない。
    pub async fn note_graph(
        &self,
        actor: &Actor,
        query: &NoteGraphQuery,
    ) -> Result<NoteGraph, SqliteStoreError> {
        let mut transaction = self.pool.begin().await.map_err(database_error)?;
        // 語の指定がない場合はすべての可視ノートを対象にする。空文字は指定なしと同じに扱う。
        let text = query
            .text
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(crate::like_contains_pattern);

        let notes = sqlx::query(
            "SELECT notes.note_id, notes.title, notes.tags_json, notes.updated_at_ms
             FROM notes
             JOIN note_access access ON access.note_id = notes.note_id
             WHERE notes.deleted_at_ms IS NULL
               AND access.principal_id = ?
               AND (?2 IS NULL
                    OR lower(notes.title) LIKE ?2 ESCAPE '!'
                    OR lower(notes.source) LIKE ?2 ESCAPE '!'
                    OR lower(notes.tags_json) LIKE ?2 ESCAPE '!')
             ORDER BY notes.note_id ASC",
        )
        .bind(actor.principal_id().get())
        .bind(text.as_deref())
        .fetch_all(&mut *transaction)
        .await
        .map_err(database_error)?;
        let notes = notes
            .into_iter()
            .map(graph_note_from_row)
            .collect::<Result<Vec<_>, _>>()?;
        let visible = notes
            .iter()
            .map(|note| note.note_id)
            .collect::<HashSet<_>>();

        let references = sqlx::query(
            "SELECT reference.source_note_id, reference.target_note_id
             FROM note_references reference
             JOIN note_access source_access ON source_access.note_id = reference.source_note_id
             JOIN note_access target_access ON target_access.note_id = reference.target_note_id
             JOIN notes source_note ON source_note.note_id = reference.source_note_id
             JOIN notes target_note ON target_note.note_id = reference.target_note_id
             WHERE source_access.principal_id = ?
               AND target_access.principal_id = ?
               AND source_note.deleted_at_ms IS NULL
               AND target_note.deleted_at_ms IS NULL
             ORDER BY reference.source_note_id ASC, reference.target_note_id ASC",
        )
        .bind(actor.principal_id().get())
        .bind(actor.principal_id().get())
        .fetch_all(&mut *transaction)
        .await
        .map_err(database_error)?;

        let citations = sqlx::query(
            "SELECT citation.source_note_id, citation.citation_key,
                    (SELECT item.csl_json FROM bibliography_items item
                      WHERE item.owner_principal_id = source_note.creator_principal_id
                        AND item.citation_key = citation.citation_key) AS csl_json
             FROM note_citations citation
             JOIN notes source_note ON source_note.note_id = citation.source_note_id
             JOIN note_access access ON access.note_id = citation.source_note_id
             WHERE access.principal_id = ?
               AND source_note.deleted_at_ms IS NULL
             ORDER BY citation.source_note_id ASC, citation.citation_key ASC",
        )
        .bind(actor.principal_id().get())
        .fetch_all(&mut *transaction)
        .await
        .map_err(database_error)?;
        transaction.commit().await.map_err(database_error)?;

        let references = references
            .into_iter()
            .map(|row| {
                Ok(NoteGraphReference {
                    source_note_id: note_id_from_text(
                        row.try_get("source_note_id").map_err(database_error)?,
                    )?,
                    target_note_id: note_id_from_text(
                        row.try_get("target_note_id").map_err(database_error)?,
                    )?,
                })
            })
            .collect::<Result<Vec<_>, SqliteStoreError>>()?
            .into_iter()
            // 語で絞り込んだ場合、両端が残っている線だけを描く。
            .filter(|edge| {
                visible.contains(&edge.source_note_id) && visible.contains(&edge.target_note_id)
            })
            .collect::<Vec<_>>();

        let mut works: BTreeMap<String, Option<String>> = BTreeMap::new();
        let mut citation_edges = Vec::new();
        for row in citations {
            let source_note_id =
                note_id_from_text(row.try_get("source_note_id").map_err(database_error)?)?;
            if !visible.contains(&source_note_id) {
                continue;
            }
            let citation_key: String = row.try_get("citation_key").map_err(database_error)?;
            let csl_json: Option<String> = row.try_get("csl_json").map_err(database_error)?;
            let title = match csl_json.as_deref() {
                Some(csl_json) => csl_title(&citation_key, csl_json)?,
                None => None,
            };
            works
                .entry(citation_key.clone())
                .and_modify(|known| {
                    if known.is_none() {
                        *known = title.clone();
                    }
                })
                .or_insert(title);
            citation_edges.push(NoteGraphCitation {
                source_note_id,
                citation_key,
            });
        }

        Ok(NoteGraph {
            notes,
            works: works
                .into_iter()
                .map(|(citation_key, title)| NoteGraphWork {
                    citation_key,
                    title,
                })
                .collect(),
            references,
            citations: citation_edges,
        })
    }
}

fn graph_note_from_row(row: sqlx::sqlite::SqliteRow) -> Result<NoteGraphNote, SqliteStoreError> {
    let tags_json: String = row.try_get("tags_json").map_err(database_error)?;
    Ok(NoteGraphNote {
        note_id: note_id_from_text(row.try_get("note_id").map_err(database_error)?)?,
        title: row.try_get("title").map_err(database_error)?,
        tags: serde_json::from_str(&tags_json).map_err(|_| SqliteStoreError::CorruptData)?,
        updated_at: UnixMillis::new(row.try_get("updated_at_ms").map_err(database_error)?),
    })
}

/// 図に出す文献の題名。CSL-JSONの`title`だけを読み、他の項目は取り出さない。
fn csl_title(citation_key: &str, csl_json: &str) -> Result<Option<String>, SqliteStoreError> {
    let csl_json = ValidatedCslJson::from_encoded(citation_key, csl_json)
        .map_err(|_| SqliteStoreError::CorruptData)?;
    Ok(csl_json
        .value()
        .get("title")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned))
}
