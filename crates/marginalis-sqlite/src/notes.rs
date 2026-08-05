//! ノート正本、所有者認可、ソフトデリートの永続化。

use std::str::FromStr;

use std::collections::{BTreeMap, HashSet};

use marginalis_application::{
    AccessibleNote, NoteAclState, NoteGraph, NoteGraphCitation, NoteGraphNote, NoteGraphQuery,
    NoteGraphReference, NoteGraphWork, NoteLinks, NoteListQuery, NoteViewSnapshot, RelatedNotes,
};
use marginalis_domain::{
    Actor, DeletedNoteListEntry, EntityId, Identity, Note, NoteAccess, NoteAclEntry,
    NoteCreationSource, NoteDraft, NoteId, NoteListEntry, NotePermission, NoteRestore,
    NoteReviewRecord, NoteReviewStatus, NoteReviewTracking, NoteSummary, Revision,
    SOFT_DELETE_RETENTION_MS, UnixMillis,
};
use sqlx::{QueryBuilder, Row, Sqlite};

use crate::{SqliteDatabase, SqliteStoreError, database_error};

/// ノート復元だけが持つ結果を、SQLite全体の共通エラーから分離する。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum RestoreNoteError {
    Store(SqliteStoreError),
    RetentionExpired,
}

impl From<SqliteStoreError> for RestoreNoteError {
    fn from(error: SqliteStoreError) -> Self {
        Self::Store(error)
    }
}

impl SqliteDatabase {
    /// 作成主体を変更不能な所有者として正本へ記録する。
    pub async fn create_note(
        &self,
        note: &Note,
        links: NoteLinks<'_>,
    ) -> Result<(), SqliteStoreError> {
        let tags_json =
            serde_json::to_string(note.tags()).map_err(|_| SqliteStoreError::CorruptData)?;
        let mut transaction = self.pool.begin().await.map_err(database_error)?;
        sqlx::query(
            "INSERT INTO notes (
                note_id, creator_issuer, creator_subject, title, source, tags_json,
                created_at_ms, updated_at_ms, revision, deleted_at_ms, created_via,
                review_tracking_known, reviewed_revision, reviewed_at_ms,
                reviewer_issuer, reviewer_subject
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(note.note_id().to_string())
        .bind(note.creator_issuer())
        .bind(note.creator_subject())
        .bind(note.title())
        .bind(note.source())
        .bind(tags_json)
        .bind(note.created_at().get())
        .bind(note.updated_at().get())
        .bind(note.revision().get())
        .bind(note.deleted_at().map(UnixMillis::get))
        .bind(note.created_via().as_str())
        .bind(i64::from(note.review_tracking_known()))
        .bind(note.last_review().map(|review| review.revision().get()))
        .bind(note.last_review().map(|review| review.reviewed_at().get()))
        .bind(note.last_review().map(|review| review.reviewer().issuer()))
        .bind(note.last_review().map(|review| review.reviewer().subject()))
        .execute(&mut *transaction)
        .await
        .map_err(database_error)?;
        replace_link_rows(&mut transaction, note.note_id(), links).await?;
        transaction.commit().await.map_err(database_error)?;
        Ok(())
    }

    #[cfg(test)]
    pub(crate) async fn note(
        &self,
        note_id: NoteId,
        include_deleted: bool,
    ) -> Result<Option<Note>, SqliteStoreError> {
        let row = if include_deleted {
            sqlx::query("SELECT * FROM notes WHERE note_id = ?")
                .bind(note_id.to_string())
                .fetch_optional(&self.pool)
                .await
        } else {
            sqlx::query("SELECT * FROM notes WHERE note_id = ? AND deleted_at_ms IS NULL")
                .bind(note_id.to_string())
                .fetch_optional(&self.pool)
                .await
        }
        .map_err(database_error)?;
        row.map(note_from_row).transpose()
    }

    /// 所有者またはACLで共有された利用者だけに、削除済みでない正本を返す。
    pub async fn accessible_note(
        &self,
        actor: &Actor,
        note_id: NoteId,
    ) -> Result<Option<AccessibleNote>, SqliteStoreError> {
        let row = sqlx::query(
            "SELECT notes.*, access.access_level
             FROM notes
             JOIN note_access access ON access.note_id = notes.note_id
             WHERE notes.note_id = ? AND notes.deleted_at_ms IS NULL
               AND access.issuer = ? AND access.subject = ?",
        )
        .bind(note_id.to_string())
        .bind(actor.issuer())
        .bind(actor.subject())
        .fetch_optional(&self.pool)
        .await
        .map_err(database_error)?;
        row.map(|row| {
            let access = access_from_level(row.try_get("access_level").map_err(database_error)?)?;
            Ok(AccessibleNote {
                note: note_from_row(row)?,
                access,
            })
        })
        .transpose()
    }

    /// 指定されたIDのうち、現在可視なノートを一括取得する。
    pub async fn visible_notes_by_id(
        &self,
        actor: &Actor,
        note_ids: &[NoteId],
    ) -> Result<Vec<Note>, SqliteStoreError> {
        if note_ids.is_empty() {
            return Ok(Vec::new());
        }
        let mut query = QueryBuilder::<Sqlite>::new(
            "SELECT *
             FROM notes
             WHERE deleted_at_ms IS NULL AND note_id IN (",
        );
        let mut separated = query.separated(", ");
        for note_id in note_ids {
            separated.push_bind(note_id.to_string());
        }
        separated.push_unseparated(
            ") AND EXISTS (SELECT 1 FROM note_access access
                           WHERE access.note_id = notes.note_id
                             AND access.issuer = ",
        );
        separated.push_bind_unseparated(actor.issuer());
        separated.push_unseparated(" AND access.subject = ");
        separated.push_bind_unseparated(actor.subject());
        separated.push_unseparated(") ORDER BY note_id");
        query
            .build()
            .fetch_all(&self.pool)
            .await
            .map_err(database_error)?
            .into_iter()
            .map(note_from_row)
            .collect()
    }

    /// 削除済みでない、所有者またはACL共有先に可視なノートを安定した順序で返す。
    pub async fn list_visible_notes(
        &self,
        actor: &Actor,
        query: &NoteListQuery,
    ) -> Result<Vec<NoteListEntry>, SqliteStoreError> {
        let rows = sqlx::query(
            "SELECT notes.note_id, notes.title, notes.tags_json, notes.updated_at_ms,
                    notes.revision, notes.created_via, notes.review_tracking_known,
                    notes.reviewed_revision, notes.reviewed_at_ms, access.access_level
             FROM notes
             JOIN note_access access ON access.note_id = notes.note_id
             WHERE notes.deleted_at_ms IS NULL
               AND access.issuer = ? AND access.subject = ?
               AND (?3 IS NULL OR notes.created_via = ?3)
               AND (
                    ?4 IS NULL
                    OR (?4 = 'unknown' AND notes.review_tracking_known = 0)
                    OR (?4 = 'pending' AND notes.review_tracking_known = 1
                        AND (notes.reviewed_revision IS NULL
                            OR notes.reviewed_revision != notes.revision))
                    OR (?4 = 'reviewed' AND notes.review_tracking_known = 1
                        AND notes.reviewed_revision = notes.revision)
               )
             ORDER BY notes.updated_at_ms DESC, notes.note_id ASC",
        )
        .bind(actor.issuer())
        .bind(actor.subject())
        .bind(query.created_via.map(NoteCreationSource::as_str))
        .bind(query.review_status.map(NoteReviewStatus::as_str))
        .fetch_all(&self.pool)
        .await
        .map_err(database_error)?;
        rows.into_iter().map(note_list_entry_from_row).collect()
    }

    /// 現在の利用者が所有する削除済みノートだけを、本文と共有先を含めずに返す。
    pub async fn list_owned_deleted_notes(
        &self,
        actor: &Actor,
    ) -> Result<Vec<DeletedNoteListEntry>, SqliteStoreError> {
        let rows = sqlx::query(
            "SELECT note_id, title, deleted_at_ms, revision
             FROM notes
             WHERE deleted_at_ms IS NOT NULL
               AND creator_issuer = ? AND creator_subject = ?
             ORDER BY deleted_at_ms DESC, note_id ASC",
        )
        .bind(actor.issuer())
        .bind(actor.subject())
        .fetch_all(&self.pool)
        .await
        .map_err(database_error)?;
        rows.into_iter().map(deleted_note_entry_from_row).collect()
    }

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
               AND access.issuer = ? AND access.subject = ?
               AND (?3 IS NULL
                    OR lower(notes.title) LIKE ?3 ESCAPE '!'
                    OR lower(notes.source) LIKE ?3 ESCAPE '!'
                    OR lower(notes.tags_json) LIKE ?3 ESCAPE '!')
             ORDER BY notes.note_id ASC",
        )
        .bind(actor.issuer())
        .bind(actor.subject())
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
             WHERE source_access.issuer = ? AND source_access.subject = ?
               AND target_access.issuer = ? AND target_access.subject = ?
               AND source_note.deleted_at_ms IS NULL
               AND target_note.deleted_at_ms IS NULL
             ORDER BY reference.source_note_id ASC, reference.target_note_id ASC",
        )
        .bind(actor.issuer())
        .bind(actor.subject())
        .bind(actor.issuer())
        .bind(actor.subject())
        .fetch_all(&mut *transaction)
        .await
        .map_err(database_error)?;

        let citations = sqlx::query(
            "SELECT citation.source_note_id, citation.citation_key,
                    (SELECT item.csl_json FROM bibliography_items item
                      WHERE item.owner_issuer = source_note.creator_issuer
                        AND item.owner_subject = source_note.creator_subject
                        AND item.citation_key = citation.citation_key) AS csl_json
             FROM note_citations citation
             JOIN notes source_note ON source_note.note_id = citation.source_note_id
             JOIN note_access access ON access.note_id = citation.source_note_id
             WHERE access.issuer = ? AND access.subject = ?
               AND source_note.deleted_at_ms IS NULL
             ORDER BY citation.source_note_id ASC, citation.citation_key ASC",
        )
        .bind(actor.issuer())
        .bind(actor.subject())
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
            let title = csl_json.as_deref().and_then(csl_title);
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

    /// 認可確認と楽観的更新を同一transactionで行う。
    pub async fn update_visible_note(
        &self,
        actor: &Actor,
        note_id: NoteId,
        expected_revision: Revision,
        draft: &NoteDraft,
        links: NoteLinks<'_>,
        updated_at: UnixMillis,
    ) -> Result<Note, SqliteStoreError> {
        let tags_json =
            serde_json::to_string(&draft.tags).map_err(|_| SqliteStoreError::CorruptData)?;
        let mut transaction = self.pool.begin().await.map_err(database_error)?;
        let result = sqlx::query(
            "UPDATE notes
             SET title = ?, source = ?, tags_json = ?, updated_at_ms = ?,
                 review_tracking_known = 1, revision = revision + 1
             WHERE note_id = ? AND revision = ? AND deleted_at_ms IS NULL
               AND EXISTS (SELECT 1 FROM note_access access
                           WHERE access.note_id = notes.note_id
                             AND access.issuer = ? AND access.subject = ?
                             AND access.access_level >= 2)",
        )
        .bind(&draft.title)
        .bind(&draft.source)
        .bind(tags_json)
        .bind(updated_at.get())
        .bind(note_id.to_string())
        .bind(expected_revision.get())
        .bind(actor.issuer())
        .bind(actor.subject())
        .execute(&mut *transaction)
        .await
        .map_err(database_error)?;
        if result.rows_affected() != 1 {
            let error =
                classify_failed_mutation(&mut transaction, actor, note_id, NoteAccess::Edit)
                    .await?;
            transaction.rollback().await.map_err(database_error)?;
            return Err(error);
        }
        let row = sqlx::query("SELECT * FROM notes WHERE note_id = ?")
            .bind(note_id.to_string())
            .fetch_one(&mut *transaction)
            .await
            .map_err(database_error)?;
        let note = note_from_row(row)?;
        replace_link_rows(&mut transaction, note_id, links).await?;
        transaction.commit().await.map_err(database_error)?;
        Ok(note)
    }

    /// 所有者の認可とソフトデリートを同一transactionで行う。
    pub async fn soft_delete_visible_note(
        &self,
        actor: &Actor,
        note_id: NoteId,
        expected_revision: Revision,
        deleted_at: UnixMillis,
    ) -> Result<Note, SqliteStoreError> {
        let mut transaction = self.pool.begin().await.map_err(database_error)?;
        let result = sqlx::query(
            "UPDATE notes
             SET deleted_at_ms = ?, updated_at_ms = ?, review_tracking_known = 1,
                 revision = revision + 1
             WHERE note_id = ? AND revision = ? AND deleted_at_ms IS NULL
               AND EXISTS (SELECT 1 FROM note_access access
                           WHERE access.note_id = notes.note_id
                             AND access.issuer = ? AND access.subject = ?
                             AND access.access_level >= 3)",
        )
        .bind(deleted_at.get())
        .bind(deleted_at.get())
        .bind(note_id.to_string())
        .bind(expected_revision.get())
        .bind(actor.issuer())
        .bind(actor.subject())
        .execute(&mut *transaction)
        .await
        .map_err(database_error)?;
        if result.rows_affected() != 1 {
            let error =
                classify_failed_mutation(&mut transaction, actor, note_id, NoteAccess::Manage)
                    .await?;
            transaction.rollback().await.map_err(database_error)?;
            return Err(error);
        }
        let row = note_row(&mut transaction, note_id).await?;
        let note = note_from_row(row)?;
        transaction.commit().await.map_err(database_error)?;
        Ok(note)
    }

    /// 所有者の認可と復元を同一transactionで行う。
    pub(crate) async fn restore_owned_deleted_note(
        &self,
        actor: &Actor,
        note_id: NoteId,
        expected_revision: Revision,
        restored_at: UnixMillis,
    ) -> Result<Note, RestoreNoteError> {
        let mut transaction = self.pool.begin().await.map_err(database_error)?;
        let retention_cutoff = restored_at.get().saturating_sub(SOFT_DELETE_RETENTION_MS);
        let result = sqlx::query(
            "UPDATE notes
             SET deleted_at_ms = NULL, updated_at_ms = ?, review_tracking_known = 1,
                 revision = revision + 1
             WHERE note_id = ? AND revision = ?
               AND deleted_at_ms IS NOT NULL AND deleted_at_ms >= ?
               AND creator_issuer = ? AND creator_subject = ?",
        )
        .bind(restored_at.get())
        .bind(note_id.to_string())
        .bind(expected_revision.get())
        .bind(retention_cutoff)
        .bind(actor.issuer())
        .bind(actor.subject())
        .execute(&mut *transaction)
        .await
        .map_err(database_error)?;
        if result.rows_affected() != 1 {
            let error = classify_failed_restore(
                &mut transaction,
                actor,
                note_id,
                expected_revision,
                retention_cutoff,
            )
            .await?;
            transaction.rollback().await.map_err(database_error)?;
            return Err(error);
        }
        let row = note_row(&mut transaction, note_id).await?;
        let note = note_from_row(row)?;
        transaction.commit().await.map_err(database_error)?;
        Ok(note)
    }

    /// retention期限を過ぎた削除済みノートを物理削除する。
    pub async fn purge_deleted_before(&self, cutoff: UnixMillis) -> Result<u64, SqliteStoreError> {
        let mut transaction = self.pool.begin().await.map_err(database_error)?;
        let result =
            sqlx::query("DELETE FROM notes WHERE deleted_at_ms IS NOT NULL AND deleted_at_ms < ?")
                .bind(cutoff.get())
                .execute(&mut *transaction)
                .await
                .map_err(database_error)?;
        transaction.commit().await.map_err(database_error)?;
        Ok(result.rows_affected())
    }

    /// 閲覧に必要な正本、権限、参照先、関連概要を一つの読み取りtransactionから取得する。
    pub async fn note_view_snapshot(
        &self,
        actor: &Actor,
        note_id: NoteId,
    ) -> Result<Option<NoteViewSnapshot>, SqliteStoreError> {
        let mut transaction = self.pool.begin().await.map_err(database_error)?;
        let row = sqlx::query(
            "SELECT notes.*, access.access_level
             FROM notes
             JOIN note_access access ON access.note_id = notes.note_id
             WHERE notes.note_id = ? AND notes.deleted_at_ms IS NULL
               AND access.issuer = ? AND access.subject = ?",
        )
        .bind(note_id.to_string())
        .bind(actor.issuer())
        .bind(actor.subject())
        .fetch_optional(&mut *transaction)
        .await
        .map_err(database_error)?;
        let Some(row) = row else {
            transaction.commit().await.map_err(database_error)?;
            return Ok(None);
        };
        let access = access_from_level(
            row.try_get::<i64, _>("access_level")
                .map_err(database_error)?,
        )?;
        let note = note_from_row(row)?;
        let reference_targets = sqlx::query(
            "SELECT target.*
             FROM note_references reference
             JOIN notes target ON target.note_id = reference.target_note_id
             WHERE reference.source_note_id = ? AND target.deleted_at_ms IS NULL
               AND EXISTS (SELECT 1 FROM note_access access
                           WHERE access.note_id = target.note_id
                             AND access.issuer = ? AND access.subject = ?)
             ORDER BY target.note_id",
        )
        .bind(note_id.to_string())
        .bind(actor.issuer())
        .bind(actor.subject())
        .fetch_all(&mut *transaction)
        .await
        .map_err(database_error)?
        .into_iter()
        .map(note_from_row)
        .collect::<Result<Vec<_>, _>>()?;
        let outgoing = reference_targets.iter().map(NoteSummary::from).collect();
        let incoming = sqlx::query(
            "SELECT source.note_id, source.title, source.tags_json, source.updated_at_ms,
                    source.revision, source.created_via, source.review_tracking_known,
                    source.reviewed_revision, source.reviewed_at_ms
             FROM note_references reference
             JOIN notes source ON source.note_id = reference.source_note_id
             WHERE reference.target_note_id = ? AND source.deleted_at_ms IS NULL
               AND EXISTS (SELECT 1 FROM note_access access
                           WHERE access.note_id = source.note_id
                             AND access.issuer = ? AND access.subject = ?)
             ORDER BY source.updated_at_ms DESC, source.note_id ASC",
        )
        .bind(note_id.to_string())
        .bind(actor.issuer())
        .bind(actor.subject())
        .fetch_all(&mut *transaction)
        .await
        .map_err(database_error)?
        .into_iter()
        .map(note_summary_from_row)
        .collect::<Result<Vec<_>, _>>()?;
        transaction.commit().await.map_err(database_error)?;
        Ok(Some(NoteViewSnapshot {
            note,
            access,
            reference_targets,
            related: RelatedNotes { outgoing, incoming },
        }))
    }

    pub async fn read_note_acl(
        &self,
        actor: &Actor,
        note_id: NoteId,
    ) -> Result<NoteAclState, SqliteStoreError> {
        let mut transaction = self.pool.begin().await.map_err(database_error)?;
        require_active_note_access(&mut transaction, actor, note_id, NoteAccess::Manage).await?;
        let rows = sqlx::query(
            "SELECT issuer, subject, permission FROM note_acl
             WHERE note_id = ? ORDER BY issuer, subject",
        )
        .bind(note_id.to_string())
        .fetch_all(&mut *transaction)
        .await
        .map_err(database_error)?;
        let entries = rows
            .into_iter()
            .map(|row| {
                let permission = match row
                    .try_get::<String, _>("permission")
                    .map_err(database_error)?
                    .as_str()
                {
                    "read" => NotePermission::Read,
                    "edit" => NotePermission::Edit,
                    _ => return Err(SqliteStoreError::CorruptData),
                };
                let identity = Identity::new(
                    row.try_get("issuer").map_err(database_error)?,
                    row.try_get("subject").map_err(database_error)?,
                )
                .map_err(|_| SqliteStoreError::CorruptData)?;
                Ok(NoteAclEntry::new(identity, permission))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let revision = Revision::new(
            sqlx::query_scalar::<_, i64>("SELECT revision FROM notes WHERE note_id = ?")
                .bind(note_id.to_string())
                .fetch_one(&mut *transaction)
                .await
                .map_err(database_error)?,
        )
        .map_err(|_| SqliteStoreError::CorruptData)?;
        transaction.commit().await.map_err(database_error)?;
        Ok(NoteAclState { entries, revision })
    }

    pub async fn replace_note_acl(
        &self,
        actor: &Actor,
        note_id: NoteId,
        entries: &[NoteAclEntry],
        expected_revision: Revision,
        updated_at: UnixMillis,
    ) -> Result<Note, SqliteStoreError> {
        let mut transaction = self.pool.begin().await.map_err(database_error)?;
        let result = sqlx::query(
            "UPDATE notes
             SET revision = revision + 1, updated_at_ms = ?, review_tracking_known = 1
             WHERE note_id = ? AND revision = ? AND deleted_at_ms IS NULL
               AND EXISTS (SELECT 1 FROM note_access access
                           WHERE access.note_id = notes.note_id
                             AND access.issuer = ? AND access.subject = ?
                             AND access.access_level >= 3)",
        )
        .bind(updated_at.get())
        .bind(note_id.to_string())
        .bind(expected_revision.get())
        .bind(actor.issuer())
        .bind(actor.subject())
        .execute(&mut *transaction)
        .await
        .map_err(database_error)?;
        if result.rows_affected() != 1 {
            let error =
                classify_failed_mutation(&mut transaction, actor, note_id, NoteAccess::Manage)
                    .await?;
            transaction.rollback().await.map_err(database_error)?;
            return Err(error);
        }
        sqlx::query("DELETE FROM note_acl WHERE note_id = ?")
            .bind(note_id.to_string())
            .execute(&mut *transaction)
            .await
            .map_err(database_error)?;
        for entry in entries {
            sqlx::query(
                "INSERT INTO note_acl (note_id, issuer, subject, permission) VALUES (?, ?, ?, ?)",
            )
            .bind(note_id.to_string())
            .bind(entry.identity().issuer())
            .bind(entry.identity().subject())
            .bind(match entry.permission() {
                NotePermission::Read => "read",
                NotePermission::Edit => "edit",
            })
            .execute(&mut *transaction)
            .await
            .map_err(database_error)?;
        }
        let note = note_from_row(note_row(&mut transaction, note_id).await?)?;
        transaction.commit().await.map_err(database_error)?;
        Ok(note)
    }
}

/// 本文が指し示す先を、ノート参照と引用の両方まとめて置き換える。
async fn replace_link_rows(
    transaction: &mut sqlx::Transaction<'_, Sqlite>,
    source_note_id: NoteId,
    links: NoteLinks<'_>,
) -> Result<(), SqliteStoreError> {
    replace_reference_rows(transaction, source_note_id, links.reference_targets).await?;
    sqlx::query("DELETE FROM note_citations WHERE source_note_id = ?")
        .bind(source_note_id.to_string())
        .execute(&mut **transaction)
        .await
        .map_err(database_error)?;
    for key in links.cited_keys {
        sqlx::query(
            "INSERT OR IGNORE INTO note_citations (source_note_id, citation_key) VALUES (?, ?)",
        )
        .bind(source_note_id.to_string())
        .bind(key)
        .execute(&mut **transaction)
        .await
        .map_err(database_error)?;
    }
    Ok(())
}

async fn replace_reference_rows(
    transaction: &mut sqlx::Transaction<'_, Sqlite>,
    source_note_id: NoteId,
    targets: &[NoteId],
) -> Result<(), SqliteStoreError> {
    sqlx::query("DELETE FROM note_references WHERE source_note_id = ?")
        .bind(source_note_id.to_string())
        .execute(&mut **transaction)
        .await
        .map_err(database_error)?;
    for target in targets {
        sqlx::query(
            "INSERT OR IGNORE INTO note_references (source_note_id, target_note_id) VALUES (?, ?)",
        )
        .bind(source_note_id.to_string())
        .bind(target.to_string())
        .execute(&mut **transaction)
        .await
        .map_err(database_error)?;
    }
    Ok(())
}

fn note_summary_from_row(row: sqlx::sqlite::SqliteRow) -> Result<NoteSummary, SqliteStoreError> {
    let note_id = NoteId::new(
        EntityId::from_str(row.try_get("note_id").map_err(database_error)?)
            .map_err(|_| SqliteStoreError::CorruptData)?,
    );
    let tags_json: String = row.try_get("tags_json").map_err(database_error)?;
    let revision = Revision::new(row.try_get("revision").map_err(database_error)?)
        .map_err(|_| SqliteStoreError::CorruptData)?;
    let review_tracking_known = row
        .try_get::<i64, _>("review_tracking_known")
        .map_err(database_error)?;
    let reviewed_revision = row
        .try_get::<Option<i64>, _>("reviewed_revision")
        .map_err(database_error)?
        .map(Revision::new)
        .transpose()
        .map_err(|_| SqliteStoreError::CorruptData)?;
    let review_status = review_status(review_tracking_known, reviewed_revision, revision)?;
    Ok(NoteSummary {
        note_id,
        title: row.try_get("title").map_err(database_error)?,
        tags: serde_json::from_str(&tags_json).map_err(|_| SqliteStoreError::CorruptData)?,
        updated_at: UnixMillis::new(row.try_get("updated_at_ms").map_err(database_error)?),
        revision,
        created_via: row
            .try_get::<String, _>("created_via")
            .map_err(database_error)?
            .parse()
            .map_err(|_| SqliteStoreError::CorruptData)?,
        review_status,
        reviewed_revision,
        reviewed_at: row
            .try_get::<Option<i64>, _>("reviewed_at_ms")
            .map_err(database_error)?
            .map(UnixMillis::new),
    })
}

fn note_list_entry_from_row(
    row: sqlx::sqlite::SqliteRow,
) -> Result<NoteListEntry, SqliteStoreError> {
    let access = access_from_level(
        row.try_get::<i64, _>("access_level")
            .map_err(database_error)?,
    )?;
    Ok(NoteListEntry {
        summary: note_summary_from_row(row)?,
        access,
    })
}

fn deleted_note_entry_from_row(
    row: sqlx::sqlite::SqliteRow,
) -> Result<DeletedNoteListEntry, SqliteStoreError> {
    let deleted_at = UnixMillis::new(row.try_get("deleted_at_ms").map_err(database_error)?);
    Ok(DeletedNoteListEntry {
        note_id: note_id_from_text(row.try_get("note_id").map_err(database_error)?)?,
        title: row.try_get("title").map_err(database_error)?,
        deleted_at,
        purge_at: UnixMillis::new(deleted_at.get().saturating_add(SOFT_DELETE_RETENTION_MS)),
        revision: Revision::new(row.try_get("revision").map_err(database_error)?)
            .map_err(|_| SqliteStoreError::CorruptData)?,
    })
}

pub(crate) async fn require_active_note_access(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    actor: &Actor,
    note_id: NoteId,
    required: NoteAccess,
) -> Result<(), SqliteStoreError> {
    let query = "SELECT 1 FROM notes
         WHERE note_id = ? AND deleted_at_ms IS NULL
           AND EXISTS (SELECT 1 FROM note_access access
                       WHERE access.note_id = notes.note_id
                         AND access.issuer = ? AND access.subject = ?
                         AND access.access_level >= ?)";
    let visible = sqlx::query_scalar::<_, i64>(query)
        .bind(note_id.to_string())
        .bind(actor.issuer())
        .bind(actor.subject())
        .bind(access_level(required))
        .fetch_optional(&mut **transaction)
        .await
        .map_err(database_error)?;
    visible.map(|_| ()).ok_or(SqliteStoreError::NotFound)
}

pub(crate) async fn classify_failed_mutation(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    actor: &Actor,
    note_id: NoteId,
    required: NoteAccess,
) -> Result<SqliteStoreError, SqliteStoreError> {
    match require_active_note_access(transaction, actor, note_id, required).await {
        Ok(()) => Ok(SqliteStoreError::Conflict),
        Err(SqliteStoreError::NotFound) => Ok(SqliteStoreError::NotFound),
        Err(error) => Err(error),
    }
}

async fn classify_failed_restore(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    actor: &Actor,
    note_id: NoteId,
    expected_revision: Revision,
    retention_cutoff: i64,
) -> Result<RestoreNoteError, SqliteStoreError> {
    let row = sqlx::query(
        "SELECT revision, deleted_at_ms
         FROM notes
         WHERE note_id = ? AND deleted_at_ms IS NOT NULL
           AND creator_issuer = ? AND creator_subject = ?",
    )
    .bind(note_id.to_string())
    .bind(actor.issuer())
    .bind(actor.subject())
    .fetch_optional(&mut **transaction)
    .await
    .map_err(database_error)?;
    let Some(row) = row else {
        return Ok(RestoreNoteError::Store(SqliteStoreError::NotFound));
    };
    let revision = Revision::new(row.try_get("revision").map_err(database_error)?)
        .map_err(|_| SqliteStoreError::CorruptData)?;
    if revision != expected_revision {
        return Ok(RestoreNoteError::Store(SqliteStoreError::Conflict));
    }
    let deleted_at: i64 = row.try_get("deleted_at_ms").map_err(database_error)?;
    if deleted_at < retention_cutoff {
        return Ok(RestoreNoteError::RetentionExpired);
    }
    Ok(RestoreNoteError::Store(SqliteStoreError::Conflict))
}

const fn access_level(access: NoteAccess) -> i64 {
    match access {
        NoteAccess::Read => 1,
        NoteAccess::Edit => 2,
        NoteAccess::Manage => 3,
    }
}

fn access_from_level(level: i64) -> Result<NoteAccess, SqliteStoreError> {
    match level {
        1 => Ok(NoteAccess::Read),
        2 => Ok(NoteAccess::Edit),
        3 => Ok(NoteAccess::Manage),
        _ => Err(SqliteStoreError::CorruptData),
    }
}

pub(crate) async fn note_row(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    note_id: NoteId,
) -> Result<sqlx::sqlite::SqliteRow, SqliteStoreError> {
    sqlx::query("SELECT * FROM notes WHERE note_id = ?")
        .bind(note_id.to_string())
        .fetch_one(&mut **transaction)
        .await
        .map_err(database_error)
}

pub(crate) fn note_from_row(row: sqlx::sqlite::SqliteRow) -> Result<Note, SqliteStoreError> {
    let note_id = row
        .try_get::<String, _>("note_id")
        .map_err(database_error)?
        .parse::<EntityId>()
        .map(NoteId::new)
        .map_err(|_| SqliteStoreError::CorruptData)?;
    let tags_json = row
        .try_get::<String, _>("tags_json")
        .map_err(database_error)?;
    let tags = serde_json::from_str(&tags_json).map_err(|_| SqliteStoreError::CorruptData)?;
    let owner = Identity::new(
        row.try_get("creator_issuer").map_err(database_error)?,
        row.try_get("creator_subject").map_err(database_error)?,
    )
    .map_err(|_| SqliteStoreError::CorruptData)?;
    let review_tracking_known = row
        .try_get::<i64, _>("review_tracking_known")
        .map_err(database_error)?;
    let reviewed_revision = row
        .try_get::<Option<i64>, _>("reviewed_revision")
        .map_err(database_error)?
        .map(Revision::new)
        .transpose()
        .map_err(|_| SqliteStoreError::CorruptData)?;
    let reviewed_at = row
        .try_get::<Option<i64>, _>("reviewed_at_ms")
        .map_err(database_error)?
        .map(UnixMillis::new);
    let reviewer_issuer = row
        .try_get::<Option<String>, _>("reviewer_issuer")
        .map_err(database_error)?;
    let reviewer_subject = row
        .try_get::<Option<String>, _>("reviewer_subject")
        .map_err(database_error)?;
    let review = match (
        review_tracking_known,
        reviewed_revision,
        reviewed_at,
        reviewer_issuer,
        reviewer_subject,
    ) {
        (0, None, None, None, None) => NoteReviewTracking::Unknown,
        (1, None, None, None, None) => NoteReviewTracking::pending(),
        (1, Some(revision), Some(reviewed_at), Some(issuer), Some(subject)) => {
            let reviewer =
                Identity::new(issuer, subject).map_err(|_| SqliteStoreError::CorruptData)?;
            NoteReviewTracking::tracked(Some(NoteReviewRecord::new(
                revision,
                reviewed_at,
                reviewer,
            )))
        }
        _ => return Err(SqliteStoreError::CorruptData),
    };
    Note::restore(NoteRestore {
        note_id,
        owner,
        draft: NoteDraft {
            title: row.try_get("title").map_err(database_error)?,
            source: row.try_get("source").map_err(database_error)?,
            tags,
        },
        created_at: UnixMillis::new(row.try_get("created_at_ms").map_err(database_error)?),
        updated_at: UnixMillis::new(row.try_get("updated_at_ms").map_err(database_error)?),
        revision: Revision::new(row.try_get("revision").map_err(database_error)?)
            .map_err(|_| SqliteStoreError::CorruptData)?,
        deleted_at: row
            .try_get::<Option<i64>, _>("deleted_at_ms")
            .map_err(database_error)?
            .map(UnixMillis::new),
        created_via: row
            .try_get::<String, _>("created_via")
            .map_err(database_error)?
            .parse()
            .map_err(|_| SqliteStoreError::CorruptData)?,
        review,
    })
    .map_err(|_| SqliteStoreError::CorruptData)
}

fn review_status(
    tracking_known: i64,
    reviewed_revision: Option<Revision>,
    current_revision: Revision,
) -> Result<NoteReviewStatus, SqliteStoreError> {
    match (tracking_known, reviewed_revision) {
        (0, None) => Ok(NoteReviewStatus::Unknown),
        (1, None) => Ok(NoteReviewStatus::Pending),
        (1, Some(reviewed)) if reviewed == current_revision => Ok(NoteReviewStatus::Reviewed),
        (1, Some(reviewed)) if reviewed < current_revision => Ok(NoteReviewStatus::Pending),
        _ => Err(SqliteStoreError::CorruptData),
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

fn note_id_from_text(value: String) -> Result<NoteId, SqliteStoreError> {
    EntityId::from_str(&value)
        .map(NoteId::new)
        .map_err(|_| SqliteStoreError::CorruptData)
}

/// 図に出す文献の題名。CSL-JSONの`title`だけを読み、他の項目は取り出さない。
fn csl_title(csl_json: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(csl_json)
        .ok()?
        .get("title")?
        .as_str()
        .map(str::to_owned)
}
