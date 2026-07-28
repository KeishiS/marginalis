//! ノート正本、所有者認可、ソフトデリートの永続化。

use std::str::FromStr;

use marginalis_application::{NoteAclState, NoteViewSnapshot, RelatedNotes};
use marginalis_domain::{
    Actor, EntityId, Identity, Note, NoteAccess, NoteAclEntry, NoteDraft, NoteId, NotePermission,
    NoteSummary, Revision, SOFT_DELETE_RETENTION_MS, UnixMillis,
};
use sqlx::{QueryBuilder, Row, Sqlite};

use crate::{SqliteDatabase, SqliteStoreError, database_error};

impl SqliteDatabase {
    /// 作成主体を変更不能な所有者として正本へ記録する。
    pub async fn create_note(
        &self,
        note: &Note,
        reference_targets: &[NoteId],
    ) -> Result<(), SqliteStoreError> {
        let tags_json =
            serde_json::to_string(note.tags()).map_err(|_| SqliteStoreError::CorruptData)?;
        let mut transaction = self.pool.begin().await.map_err(database_error)?;
        sqlx::query(
            "INSERT INTO notes (note_id, creator_issuer, creator_subject, title, source, tags_json, created_at_ms, updated_at_ms, revision, deleted_at_ms)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
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
        .execute(&mut *transaction)
        .await
        .map_err(database_error)?;
        replace_reference_rows(&mut transaction, note.note_id(), reference_targets).await?;
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
            sqlx::query(
                "SELECT note_id, creator_issuer, creator_subject, title, source, tags_json, created_at_ms, updated_at_ms, revision, deleted_at_ms
                 FROM notes WHERE note_id = ?",
            )
            .bind(note_id.to_string())
            .fetch_optional(&self.pool)
            .await
        } else {
            sqlx::query(
                "SELECT note_id, creator_issuer, creator_subject, title, source, tags_json, created_at_ms, updated_at_ms, revision, deleted_at_ms
                 FROM notes WHERE note_id = ? AND deleted_at_ms IS NULL",
            )
            .bind(note_id.to_string())
            .fetch_optional(&self.pool)
            .await
        }
        .map_err(database_error)?;
        row.map(note_from_row).transpose()
    }

    /// 所有者またはACLで共有された利用者だけに、削除済みでない正本を返す。
    pub async fn visible_note(
        &self,
        actor: &Actor,
        note_id: NoteId,
    ) -> Result<Option<Note>, SqliteStoreError> {
        let row = sqlx::query(
            "SELECT note_id, creator_issuer, creator_subject, title, source, tags_json, created_at_ms, updated_at_ms, revision, deleted_at_ms
             FROM notes
             WHERE note_id = ? AND deleted_at_ms IS NULL
               AND EXISTS (SELECT 1 FROM note_access access
                           WHERE access.note_id = notes.note_id
                             AND access.issuer = ? AND access.subject = ?)",
        )
        .bind(note_id.to_string())
        .bind(actor.issuer())
        .bind(actor.subject())
        .fetch_optional(&self.pool)
        .await
        .map_err(database_error)?;
        row.map(note_from_row).transpose()
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
            "SELECT note_id, creator_issuer, creator_subject, title, source, tags_json,
                    created_at_ms, updated_at_ms, revision, deleted_at_ms
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
    ) -> Result<Vec<NoteSummary>, SqliteStoreError> {
        let rows = sqlx::query(
            "SELECT note_id, title, tags_json, updated_at_ms, revision
             FROM notes
             WHERE deleted_at_ms IS NULL
               AND EXISTS (SELECT 1 FROM note_access access
                           WHERE access.note_id = notes.note_id
                             AND access.issuer = ? AND access.subject = ?)
             ORDER BY updated_at_ms DESC, note_id ASC",
        )
        .bind(actor.issuer())
        .bind(actor.subject())
        .fetch_all(&self.pool)
        .await
        .map_err(database_error)?;
        rows.into_iter().map(note_summary_from_row).collect()
    }

    /// 認可確認と楽観的更新を同一transactionで行う。
    pub async fn update_visible_note(
        &self,
        actor: &Actor,
        note_id: NoteId,
        expected_revision: Revision,
        draft: &NoteDraft,
        reference_targets: &[NoteId],
        updated_at: UnixMillis,
    ) -> Result<Note, SqliteStoreError> {
        let tags_json =
            serde_json::to_string(&draft.tags).map_err(|_| SqliteStoreError::CorruptData)?;
        let mut transaction = self.pool.begin().await.map_err(database_error)?;
        let result = sqlx::query(
            "UPDATE notes
             SET title = ?, source = ?, tags_json = ?, updated_at_ms = ?, revision = revision + 1
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
            return Err(classify_failed_mutation(
                &mut transaction,
                actor,
                note_id,
                NoteDeletionState::Active,
                NoteAccess::Edit,
            )
            .await?);
        }
        let row = sqlx::query(
            "SELECT note_id, creator_issuer, creator_subject, title, source, tags_json, created_at_ms, updated_at_ms, revision, deleted_at_ms
             FROM notes WHERE note_id = ?",
        )
        .bind(note_id.to_string())
        .fetch_one(&mut *transaction)
        .await
        .map_err(database_error)?;
        let note = note_from_row(row)?;
        replace_reference_rows(&mut transaction, note_id, reference_targets).await?;
        transaction.commit().await.map_err(database_error)?;
        Ok(note)
    }

    /// 現在可視なノートだけを対象に、直接参照先と参照元の概要を返す。
    pub async fn directly_related_notes(
        &self,
        actor: &Actor,
        note_id: NoteId,
    ) -> Result<(Vec<NoteSummary>, Vec<NoteSummary>), SqliteStoreError> {
        let outgoing = sqlx::query(
            "SELECT target.note_id, target.title, target.tags_json, target.updated_at_ms,
                    target.revision
             FROM note_references reference
             JOIN notes target ON target.note_id = reference.target_note_id
             WHERE reference.source_note_id = ?
               AND target.deleted_at_ms IS NULL
               AND EXISTS (SELECT 1 FROM note_access access
                           WHERE access.note_id = target.note_id
                             AND access.issuer = ? AND access.subject = ?)
             ORDER BY target.updated_at_ms DESC, target.note_id ASC",
        )
        .bind(note_id.to_string())
        .bind(actor.issuer())
        .bind(actor.subject())
        .fetch_all(&self.pool)
        .await
        .map_err(database_error)?
        .into_iter()
        .map(note_summary_from_row)
        .collect::<Result<Vec<_>, _>>()?;
        let incoming = sqlx::query(
            "SELECT source.note_id, source.title, source.tags_json, source.updated_at_ms,
                    source.revision
             FROM note_references reference
             JOIN notes source ON source.note_id = reference.source_note_id
             WHERE reference.target_note_id = ?
               AND source.deleted_at_ms IS NULL
               AND EXISTS (SELECT 1 FROM note_access access
                           WHERE access.note_id = source.note_id
                             AND access.issuer = ? AND access.subject = ?)
             ORDER BY source.updated_at_ms DESC, source.note_id ASC",
        )
        .bind(note_id.to_string())
        .bind(actor.issuer())
        .bind(actor.subject())
        .fetch_all(&self.pool)
        .await
        .map_err(database_error)?
        .into_iter()
        .map(note_summary_from_row)
        .collect::<Result<Vec<_>, _>>()?;
        Ok((outgoing, incoming))
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
            "UPDATE notes SET deleted_at_ms = ?, updated_at_ms = ?, revision = revision + 1
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
            return Err(classify_failed_mutation(
                &mut transaction,
                actor,
                note_id,
                NoteDeletionState::Active,
                NoteAccess::Manage,
            )
            .await?);
        }
        let row = note_row(&mut transaction, note_id).await?;
        let note = note_from_row(row)?;
        transaction.commit().await.map_err(database_error)?;
        Ok(note)
    }

    /// 所有者の認可と復元を同一transactionで行う。
    pub async fn restore_visible_note(
        &self,
        actor: &Actor,
        note_id: NoteId,
        expected_revision: Revision,
        restored_at: UnixMillis,
    ) -> Result<Note, SqliteStoreError> {
        let mut transaction = self.pool.begin().await.map_err(database_error)?;
        let retention_cutoff = restored_at.get().saturating_sub(SOFT_DELETE_RETENTION_MS);
        let result = sqlx::query(
            "UPDATE notes SET deleted_at_ms = NULL, updated_at_ms = ?, revision = revision + 1
             WHERE note_id = ? AND revision = ?
               AND deleted_at_ms IS NOT NULL AND deleted_at_ms >= ?
               AND EXISTS (SELECT 1 FROM note_access access
                           WHERE access.note_id = notes.note_id
                             AND access.issuer = ? AND access.subject = ?
                             AND access.access_level >= 3)",
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
            return Err(classify_failed_mutation(
                &mut transaction,
                actor,
                note_id,
                NoteDeletionState::Deleted,
                NoteAccess::Manage,
            )
            .await?);
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

    pub async fn note_access(
        &self,
        actor: &Actor,
        note_id: NoteId,
    ) -> Result<Option<NoteAccess>, SqliteStoreError> {
        let access = sqlx::query_scalar::<_, i64>(
            "SELECT access.access_level
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
        access.map(access_from_level).transpose()
    }

    /// 閲覧に必要な正本、権限、参照先、関連概要を一つの読み取りtransactionから取得する。
    pub async fn note_view_snapshot(
        &self,
        actor: &Actor,
        note_id: NoteId,
    ) -> Result<Option<NoteViewSnapshot>, SqliteStoreError> {
        let mut transaction = self.pool.begin().await.map_err(database_error)?;
        let row = sqlx::query(
            "SELECT notes.note_id, notes.creator_issuer, notes.creator_subject, notes.title,
                    notes.source, notes.tags_json, notes.created_at_ms, notes.updated_at_ms,
                    notes.revision, notes.deleted_at_ms, access.access_level
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
            "SELECT target.note_id, target.creator_issuer, target.creator_subject, target.title,
                    target.source, target.tags_json, target.created_at_ms, target.updated_at_ms,
                    target.revision, target.deleted_at_ms
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
                    source.revision
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
        require_note_access(
            &mut transaction,
            actor,
            note_id,
            NoteDeletionState::Active,
            NoteAccess::Manage,
        )
        .await?;
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
            "UPDATE notes SET revision = revision + 1, updated_at_ms = ?
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
            return Err(classify_failed_mutation(
                &mut transaction,
                actor,
                note_id,
                NoteDeletionState::Active,
                NoteAccess::Manage,
            )
            .await?);
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
    Ok(NoteSummary {
        note_id,
        title: row.try_get("title").map_err(database_error)?,
        tags: serde_json::from_str(&tags_json).map_err(|_| SqliteStoreError::CorruptData)?,
        updated_at: UnixMillis::new(row.try_get("updated_at_ms").map_err(database_error)?),
        revision: Revision::new(row.try_get("revision").map_err(database_error)?)
            .map_err(|_| SqliteStoreError::CorruptData)?,
    })
}

#[derive(Clone, Copy)]
enum NoteDeletionState {
    Active,
    Deleted,
}

async fn require_note_access(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    actor: &Actor,
    note_id: NoteId,
    deletion_state: NoteDeletionState,
    required: NoteAccess,
) -> Result<(), SqliteStoreError> {
    let deletion_predicate = match deletion_state {
        NoteDeletionState::Active => "deleted_at_ms IS NULL",
        NoteDeletionState::Deleted => "deleted_at_ms IS NOT NULL",
    };
    let query = format!(
        "SELECT 1 FROM notes
         WHERE note_id = ? AND {deletion_predicate}
           AND EXISTS (SELECT 1 FROM note_access access
                       WHERE access.note_id = notes.note_id
                         AND access.issuer = ? AND access.subject = ?
                         AND access.access_level >= ?)"
    );
    let visible = sqlx::query_scalar::<_, i64>(&query)
        .bind(note_id.to_string())
        .bind(actor.issuer())
        .bind(actor.subject())
        .bind(access_level(required))
        .fetch_optional(&mut **transaction)
        .await
        .map_err(database_error)?;
    visible.map(|_| ()).ok_or(SqliteStoreError::NotFound)
}

async fn classify_failed_mutation(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    actor: &Actor,
    note_id: NoteId,
    deletion_state: NoteDeletionState,
    required: NoteAccess,
) -> Result<SqliteStoreError, SqliteStoreError> {
    match require_note_access(transaction, actor, note_id, deletion_state, required).await {
        Ok(()) => Ok(SqliteStoreError::Conflict),
        Err(SqliteStoreError::NotFound) => Ok(SqliteStoreError::NotFound),
        Err(error) => Err(error),
    }
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

async fn note_row(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    note_id: NoteId,
) -> Result<sqlx::sqlite::SqliteRow, SqliteStoreError> {
    sqlx::query(
        "SELECT note_id, creator_issuer, creator_subject, title, source, tags_json, created_at_ms, updated_at_ms, revision, deleted_at_ms
         FROM notes WHERE note_id = ?",
    )
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
    Note::restore(
        note_id,
        owner,
        row.try_get("title").map_err(database_error)?,
        row.try_get("source").map_err(database_error)?,
        tags,
        UnixMillis::new(row.try_get("created_at_ms").map_err(database_error)?),
        UnixMillis::new(row.try_get("updated_at_ms").map_err(database_error)?),
        Revision::new(row.try_get("revision").map_err(database_error)?)
            .map_err(|_| SqliteStoreError::CorruptData)?,
        row.try_get::<Option<i64>, _>("deleted_at_ms")
            .map_err(database_error)?
            .map(UnixMillis::new),
    )
    .map_err(|_| SqliteStoreError::CorruptData)
}
