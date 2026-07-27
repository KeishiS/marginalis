//! ノート正本、所有者認可、ソフトデリートの永続化。

use marginalis_domain::{
    Actor, EntityId, Note, NoteDraft, NoteId, SOFT_DELETE_RETENTION_MS, UnixMillis,
};
use sqlx::Row;

use crate::{SqliteDatabase, SqliteStoreError, database_error};

impl SqliteDatabase {
    /// 作成主体を変更不能な所有者として正本へ記録する。
    pub async fn create_note(&self, note: &Note) -> Result<(), SqliteStoreError> {
        let tags_json =
            serde_json::to_string(&note.tags).map_err(|_| SqliteStoreError::CorruptData)?;
        sqlx::query(
            "INSERT INTO notes (note_id, creator_issuer, creator_subject, title, body, tags_json, created_at_ms, updated_at_ms, revision, deleted_at_ms)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(note.note_id.to_string())
        .bind(&note.creator_issuer)
        .bind(&note.creator_subject)
        .bind(&note.title)
        .bind(&note.body)
        .bind(tags_json)
        .bind(note.created_at.get())
        .bind(note.updated_at.get())
        .bind(note.revision)
        .bind(note.deleted_at.map(UnixMillis::get))
        .execute(&self.pool)
        .await
        .map_err(database_error)?;
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
                "SELECT note_id, creator_issuer, creator_subject, title, body, tags_json, created_at_ms, updated_at_ms, revision, deleted_at_ms
                 FROM notes WHERE note_id = ?",
            )
            .bind(note_id.to_string())
            .fetch_optional(&self.pool)
            .await
        } else {
            sqlx::query(
                "SELECT note_id, creator_issuer, creator_subject, title, body, tags_json, created_at_ms, updated_at_ms, revision, deleted_at_ms
                 FROM notes WHERE note_id = ? AND deleted_at_ms IS NULL",
            )
            .bind(note_id.to_string())
            .fetch_optional(&self.pool)
            .await
        }
        .map_err(database_error)?;
        row.map(note_from_row).transpose()
    }

    /// 管理者または所有者だけに、削除済みでない正本を返す。
    pub async fn visible_note(
        &self,
        actor: &Actor,
        note_id: NoteId,
    ) -> Result<Option<Note>, SqliteStoreError> {
        let row = sqlx::query(
            "SELECT note_id, creator_issuer, creator_subject, title, body, tags_json, created_at_ms, updated_at_ms, revision, deleted_at_ms
             FROM notes
             WHERE note_id = ? AND deleted_at_ms IS NULL
               AND (? OR (creator_issuer = ? AND creator_subject = ?))",
        )
        .bind(note_id.to_string())
        .bind(actor.is_administrator)
        .bind(&actor.issuer)
        .bind(&actor.subject)
        .fetch_optional(&self.pool)
        .await
        .map_err(database_error)?;
        row.map(note_from_row).transpose()
    }

    /// 削除済みでない、管理者または所有者に可視なノートを安定した順序で返す。
    pub async fn list_visible_notes(&self, actor: &Actor) -> Result<Vec<Note>, SqliteStoreError> {
        let rows = sqlx::query(
            "SELECT note_id, creator_issuer, creator_subject, title, body, tags_json, created_at_ms, updated_at_ms, revision, deleted_at_ms
             FROM notes
             WHERE deleted_at_ms IS NULL
               AND (? OR (creator_issuer = ? AND creator_subject = ?))
             ORDER BY updated_at_ms DESC, note_id ASC",
        )
        .bind(actor.is_administrator)
        .bind(&actor.issuer)
        .bind(&actor.subject)
        .fetch_all(&self.pool)
        .await
        .map_err(database_error)?;
        rows.into_iter().map(note_from_row).collect()
    }

    /// 認可確認と楽観的更新を同一transactionで行う。
    pub async fn update_visible_note(
        &self,
        actor: &Actor,
        note_id: NoteId,
        expected_revision: i64,
        draft: &NoteDraft,
        updated_at: UnixMillis,
    ) -> Result<Note, SqliteStoreError> {
        let tags_json =
            serde_json::to_string(&draft.tags).map_err(|_| SqliteStoreError::CorruptData)?;
        let mut transaction = self.pool.begin().await.map_err(database_error)?;
        require_note_owner_or_administrator(
            &mut transaction,
            actor,
            note_id,
            NoteDeletionState::Active,
        )
        .await?;
        let result = sqlx::query(
            "UPDATE notes
             SET title = ?, body = ?, tags_json = ?, updated_at_ms = ?, revision = revision + 1
             WHERE note_id = ? AND revision = ? AND deleted_at_ms IS NULL",
        )
        .bind(&draft.title)
        .bind(&draft.body)
        .bind(tags_json)
        .bind(updated_at.get())
        .bind(note_id.to_string())
        .bind(expected_revision)
        .execute(&mut *transaction)
        .await
        .map_err(database_error)?;
        if result.rows_affected() != 1 {
            return Err(SqliteStoreError::Conflict);
        }
        let row = sqlx::query(
            "SELECT note_id, creator_issuer, creator_subject, title, body, tags_json, created_at_ms, updated_at_ms, revision, deleted_at_ms
             FROM notes WHERE note_id = ?",
        )
        .bind(note_id.to_string())
        .fetch_one(&mut *transaction)
        .await
        .map_err(database_error)?;
        let note = note_from_row(row)?;
        transaction.commit().await.map_err(database_error)?;
        Ok(note)
    }

    /// 所有者または管理者の認可とソフトデリートを同一transactionで行う。
    pub async fn soft_delete_visible_note(
        &self,
        actor: &Actor,
        note_id: NoteId,
        expected_revision: i64,
        deleted_at: UnixMillis,
    ) -> Result<Note, SqliteStoreError> {
        let mut transaction = self.pool.begin().await.map_err(database_error)?;
        require_note_owner_or_administrator(
            &mut transaction,
            actor,
            note_id,
            NoteDeletionState::Active,
        )
        .await?;
        let result = sqlx::query(
            "UPDATE notes SET deleted_at_ms = ?, updated_at_ms = ?, revision = revision + 1
             WHERE note_id = ? AND revision = ? AND deleted_at_ms IS NULL",
        )
        .bind(deleted_at.get())
        .bind(deleted_at.get())
        .bind(note_id.to_string())
        .bind(expected_revision)
        .execute(&mut *transaction)
        .await
        .map_err(database_error)?;
        if result.rows_affected() != 1 {
            return Err(SqliteStoreError::Conflict);
        }
        let row = note_row(&mut transaction, note_id).await?;
        let note = note_from_row(row)?;
        transaction.commit().await.map_err(database_error)?;
        Ok(note)
    }

    /// 所有者または管理者の認可と復元を同一transactionで行う。
    pub async fn restore_visible_note(
        &self,
        actor: &Actor,
        note_id: NoteId,
        expected_revision: i64,
        restored_at: UnixMillis,
    ) -> Result<Note, SqliteStoreError> {
        let mut transaction = self.pool.begin().await.map_err(database_error)?;
        require_note_owner_or_administrator(
            &mut transaction,
            actor,
            note_id,
            NoteDeletionState::Deleted,
        )
        .await?;
        let retention_cutoff = restored_at.get().saturating_sub(SOFT_DELETE_RETENTION_MS);
        let result = sqlx::query(
            "UPDATE notes SET deleted_at_ms = NULL, updated_at_ms = ?, revision = revision + 1
             WHERE note_id = ? AND revision = ?
               AND deleted_at_ms IS NOT NULL AND deleted_at_ms >= ?",
        )
        .bind(restored_at.get())
        .bind(note_id.to_string())
        .bind(expected_revision)
        .bind(retention_cutoff)
        .execute(&mut *transaction)
        .await
        .map_err(database_error)?;
        if result.rows_affected() != 1 {
            return Err(SqliteStoreError::Conflict);
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
}

enum NoteDeletionState {
    Active,
    Deleted,
}

async fn require_note_owner_or_administrator(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    actor: &Actor,
    note_id: NoteId,
    deletion_state: NoteDeletionState,
) -> Result<(), SqliteStoreError> {
    let deletion_predicate = match deletion_state {
        NoteDeletionState::Active => "deleted_at_ms IS NULL",
        NoteDeletionState::Deleted => "deleted_at_ms IS NOT NULL",
    };
    let query = format!(
        "SELECT 1 FROM notes
         WHERE note_id = ? AND {deletion_predicate}
           AND (? OR (creator_issuer = ? AND creator_subject = ?))"
    );
    let visible = sqlx::query_scalar::<_, i64>(&query)
        .bind(note_id.to_string())
        .bind(actor.is_administrator)
        .bind(&actor.issuer)
        .bind(&actor.subject)
        .fetch_optional(&mut **transaction)
        .await
        .map_err(database_error)?;
    visible.map(|_| ()).ok_or(SqliteStoreError::NotFound)
}

async fn note_row(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    note_id: NoteId,
) -> Result<sqlx::sqlite::SqliteRow, SqliteStoreError> {
    sqlx::query(
        "SELECT note_id, creator_issuer, creator_subject, title, body, tags_json, created_at_ms, updated_at_ms, revision, deleted_at_ms
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
    Ok(Note {
        note_id,
        creator_issuer: row.try_get("creator_issuer").map_err(database_error)?,
        creator_subject: row.try_get("creator_subject").map_err(database_error)?,
        title: row.try_get("title").map_err(database_error)?,
        body: row.try_get("body").map_err(database_error)?,
        tags,
        created_at: UnixMillis::new(row.try_get("created_at_ms").map_err(database_error)?),
        updated_at: UnixMillis::new(row.try_get("updated_at_ms").map_err(database_error)?),
        revision: row.try_get("revision").map_err(database_error)?,
        deleted_at: row
            .try_get::<Option<i64>, _>("deleted_at_ms")
            .map_err(database_error)?
            .map(UnixMillis::new),
    })
}
