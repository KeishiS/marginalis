//! ノート正本、直接ACL、ソフトデリートの永続化。

use marginalis_domain::{
    Actor, EntityId, Note, NoteAclEntry, NoteDraft, NoteId, NotePermission,
    SOFT_DELETE_RETENTION_MS, UnixMillis,
};
use sqlx::Row;

use crate::{SqliteDatabase, SqliteStoreError, database_error};

impl SqliteDatabase {
    /// 正本と直接ACLを同一transactionで作成する。
    pub async fn create_note(
        &self,
        note: &Note,
        owner_permission: NotePermission,
    ) -> Result<(), SqliteStoreError> {
        let tags_json =
            serde_json::to_string(&note.tags).map_err(|_| SqliteStoreError::CorruptNote)?;
        let mut transaction = self.pool.begin().await.map_err(database_error)?;
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
        .execute(&mut *transaction)
        .await
        .map_err(database_error)?;
        sqlx::query(
            "INSERT INTO note_acl (note_id, issuer, subject, permission) VALUES (?, ?, ?, ?)",
        )
        .bind(note.note_id.to_string())
        .bind(&note.creator_issuer)
        .bind(&note.creator_subject)
        .bind(permission_to_storage(owner_permission))
        .execute(&mut *transaction)
        .await
        .map_err(database_error)?;
        transaction.commit().await.map_err(database_error)
    }

    pub async fn note(
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

    /// 管理者または直接ACLを持つ主体だけに、削除済みでない正本を返す。
    pub async fn visible_note(
        &self,
        actor: &Actor,
        note_id: NoteId,
        required: NotePermission,
    ) -> Result<Option<Note>, SqliteStoreError> {
        let row = sqlx::query(
            "SELECT note_id, creator_issuer, creator_subject, title, body, tags_json, created_at_ms, updated_at_ms, revision, deleted_at_ms
             FROM notes
             WHERE note_id = ? AND deleted_at_ms IS NULL
               AND (? OR EXISTS (
                    SELECT 1 FROM note_acl
                    WHERE note_acl.note_id = notes.note_id
                      AND issuer = ? AND subject = ? AND permission >= ?
               ))",
        )
        .bind(note_id.to_string())
        .bind(actor.is_administrator)
        .bind(&actor.issuer)
        .bind(&actor.subject)
        .bind(permission_to_storage(required))
        .fetch_optional(&self.pool)
        .await
        .map_err(database_error)?;
        row.map(note_from_row).transpose()
    }

    /// 復元候補として削除済みノートをAdminだけへ返す。
    pub async fn visible_deleted_note(
        &self,
        actor: &Actor,
        note_id: NoteId,
    ) -> Result<Option<Note>, SqliteStoreError> {
        let row = sqlx::query(
            "SELECT note_id, creator_issuer, creator_subject, title, body, tags_json, created_at_ms, updated_at_ms, revision, deleted_at_ms
             FROM notes
             WHERE note_id = ? AND deleted_at_ms IS NOT NULL
               AND (? OR EXISTS (
                    SELECT 1 FROM note_acl
                    WHERE note_acl.note_id = notes.note_id
                      AND issuer = ? AND subject = ? AND permission >= ?
               ))",
        )
        .bind(note_id.to_string())
        .bind(actor.is_administrator)
        .bind(&actor.issuer)
        .bind(&actor.subject)
        .bind(permission_to_storage(NotePermission::Admin))
        .fetch_optional(&self.pool)
        .await
        .map_err(database_error)?;
        row.map(note_from_row).transpose()
    }

    /// 削除済みでない、主体に可視なノートを安定した順序で返す。
    pub async fn list_visible_notes(
        &self,
        actor: &Actor,
        offset: u64,
        limit: u32,
    ) -> Result<Vec<Note>, SqliteStoreError> {
        let rows = sqlx::query(
            "SELECT note_id, creator_issuer, creator_subject, title, body, tags_json, created_at_ms, updated_at_ms, revision, deleted_at_ms
             FROM notes
             WHERE deleted_at_ms IS NULL
               AND (? OR EXISTS (
                    SELECT 1 FROM note_acl
                    WHERE note_acl.note_id = notes.note_id
                      AND issuer = ? AND subject = ? AND permission >= ?
               ))
             ORDER BY updated_at_ms DESC, note_id ASC LIMIT ? OFFSET ?",
        )
        .bind(actor.is_administrator)
        .bind(&actor.issuer)
        .bind(&actor.subject)
        .bind(permission_to_storage(NotePermission::Read))
        .bind(i64::from(limit))
        .bind(i64::try_from(offset).unwrap_or(i64::MAX))
        .fetch_all(&self.pool)
        .await
        .map_err(database_error)?;
        rows.into_iter().map(note_from_row).collect()
    }

    /// 削除済みでない正本を楽観的ロックして更新する。
    pub async fn update_note(
        &self,
        note_id: NoteId,
        expected_revision: i64,
        draft: &NoteDraft,
        updated_at: UnixMillis,
    ) -> Result<Note, SqliteStoreError> {
        let tags_json =
            serde_json::to_string(&draft.tags).map_err(|_| SqliteStoreError::CorruptNote)?;
        let mut transaction = self.pool.begin().await.map_err(database_error)?;
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

    /// ノートを通常の閲覧経路から除外し、30日間の復元候補にする。
    pub async fn soft_delete_note(
        &self,
        note_id: NoteId,
        expected_revision: i64,
        deleted_at: UnixMillis,
    ) -> Result<(), SqliteStoreError> {
        let mut transaction = self.pool.begin().await.map_err(database_error)?;
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
        transaction.commit().await.map_err(database_error)
    }

    /// 削除から30日以内のノートを復元する。
    pub async fn restore_note(
        &self,
        note_id: NoteId,
        expected_revision: i64,
        restored_at: UnixMillis,
    ) -> Result<Note, SqliteStoreError> {
        let mut transaction = self.pool.begin().await.map_err(database_error)?;
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

    /// 直接ACLを置き換える。最後の直接Adminの降格・削除は同じtransactionで拒否する。
    pub async fn set_note_permission(
        &self,
        note_id: NoteId,
        issuer: &str,
        subject: &str,
        permission: Option<NotePermission>,
    ) -> Result<(), SqliteStoreError> {
        let mut transaction = self.pool.begin().await.map_err(database_error)?;
        let current_permission = sqlx::query_scalar::<_, i64>(
            "SELECT permission FROM note_acl WHERE note_id = ? AND issuer = ? AND subject = ?",
        )
        .bind(note_id.to_string())
        .bind(issuer)
        .bind(subject)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(database_error)?;
        let current_is_admin = matches!(current_permission, Some(3));
        if current_is_admin && permission != Some(NotePermission::Admin) {
            let administrator_count = sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM note_acl WHERE note_id = ? AND permission = 3",
            )
            .bind(note_id.to_string())
            .fetch_one(&mut *transaction)
            .await
            .map_err(database_error)?;
            if administrator_count <= 1 {
                return Err(SqliteStoreError::LastAdmin);
            }
        }
        match permission {
            Some(permission) => {
                sqlx::query(
                    "INSERT INTO note_acl (note_id, issuer, subject, permission) VALUES (?, ?, ?, ?)
                     ON CONFLICT (note_id, issuer, subject) DO UPDATE SET permission = excluded.permission",
                )
                .bind(note_id.to_string())
                .bind(issuer)
                .bind(subject)
                .bind(permission_to_storage(permission))
                .execute(&mut *transaction)
                .await
                .map_err(database_error)?;
            }
            None => {
                sqlx::query(
                    "DELETE FROM note_acl WHERE note_id = ? AND issuer = ? AND subject = ?",
                )
                .bind(note_id.to_string())
                .bind(issuer)
                .bind(subject)
                .execute(&mut *transaction)
                .await
                .map_err(database_error)?;
            }
        }
        transaction.commit().await.map_err(database_error)
    }

    pub async fn note_acl(&self, note_id: NoteId) -> Result<Vec<NoteAclEntry>, SqliteStoreError> {
        let rows = sqlx::query(
            "SELECT issuer, subject, permission FROM note_acl
             WHERE note_id = ? ORDER BY issuer ASC, subject ASC",
        )
        .bind(note_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(database_error)?;
        rows.into_iter()
            .map(|row| {
                let permission = row
                    .try_get::<i64, _>("permission")
                    .map_err(database_error)
                    .and_then(permission_from_storage)?;
                Ok(NoteAclEntry {
                    issuer: row.try_get("issuer").map_err(database_error)?,
                    subject: row.try_get("subject").map_err(database_error)?,
                    permission,
                })
            })
            .collect()
    }
}

pub(crate) fn note_from_row(row: sqlx::sqlite::SqliteRow) -> Result<Note, SqliteStoreError> {
    let note_id = row
        .try_get::<String, _>("note_id")
        .map_err(database_error)?
        .parse::<EntityId>()
        .map(NoteId::new)
        .map_err(|_| SqliteStoreError::CorruptNote)?;
    let tags_json = row
        .try_get::<String, _>("tags_json")
        .map_err(database_error)?;
    let tags = serde_json::from_str(&tags_json).map_err(|_| SqliteStoreError::CorruptNote)?;
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

fn permission_from_storage(value: i64) -> Result<NotePermission, SqliteStoreError> {
    match value {
        1 => Ok(NotePermission::Read),
        2 => Ok(NotePermission::Write),
        3 => Ok(NotePermission::Admin),
        _ => Err(SqliteStoreError::CorruptNote),
    }
}

pub(crate) fn permission_to_storage(value: NotePermission) -> i64 {
    match value {
        NotePermission::Read => 1,
        NotePermission::Write => 2,
        NotePermission::Admin => 3,
    }
}
