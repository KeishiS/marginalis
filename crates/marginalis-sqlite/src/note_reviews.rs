//! 所有者によるノートの人手確認のSQLite transaction。

use marginalis_domain::{Actor, Note, NoteAccess, NoteId, NoteRevisionKind, Revision, UnixMillis};

use crate::{
    SqliteDatabase, SqliteStoreError, database_error,
    note_history::insert_note_revision,
    notes::{classify_failed_mutation, note_from_row, note_row, require_active_note_access},
};

impl SqliteDatabase {
    /// 所有者だけに、確認者を含む人手確認情報を返す。
    pub async fn read_owned_note_review(
        &self,
        actor: &Actor,
        note_id: NoteId,
    ) -> Result<Note, SqliteStoreError> {
        let mut transaction = self.pool.begin().await.map_err(database_error)?;
        require_active_note_access(&mut transaction, actor, note_id, NoteAccess::Manage).await?;
        let note = note_from_row(note_row(&mut transaction, note_id).await?)?;
        transaction.commit().await.map_err(database_error)?;
        Ok(note)
    }

    /// 現在のrevisionを所有者が確認済みにし、確認操作自体を新しいrevisionとして記録する。
    pub async fn mark_owned_note_reviewed(
        &self,
        actor: &Actor,
        note_id: NoteId,
        expected_revision: Revision,
        reviewed_at: UnixMillis,
    ) -> Result<Note, SqliteStoreError> {
        let mut transaction = self.pool.begin().await.map_err(database_error)?;
        let result = sqlx::query(
            "UPDATE notes
             SET review_tracking_known = 1,
                 reviewed_revision = revision + 1,
                 reviewed_at_ms = ?,
                 reviewer_principal_id = ?,
                 updated_at_ms = ?,
                 revision = revision + 1
             WHERE note_id = ? AND revision = ? AND deleted_at_ms IS NULL
               AND EXISTS (SELECT 1 FROM note_access access
                           WHERE access.note_id = notes.note_id
                             AND access.principal_id = ?
                             AND access.access_level >= 3)",
        )
        .bind(reviewed_at.get())
        .bind(actor.principal_id().get())
        .bind(reviewed_at.get())
        .bind(note_id.to_string())
        .bind(expected_revision.get())
        .bind(actor.principal_id().get())
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
        let note = note_from_row(note_row(&mut transaction, note_id).await?)?;
        insert_note_revision(
            &mut transaction,
            note_id,
            actor.principal_id(),
            NoteRevisionKind::Reviewed,
        )
        .await?;
        transaction.commit().await.map_err(database_error)?;
        Ok(note)
    }
}
