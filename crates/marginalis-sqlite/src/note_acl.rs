//! ノートACLの読み取りと置き換えの永続化。

use marginalis_application::NoteAclState;
use marginalis_domain::{
    Actor, Identity, Note, NoteAccess, NoteAclEntry, NoteId, NotePermission, NoteRevisionKind,
    PrincipalId, PrincipalRef, Revision, UnixMillis,
};
use sqlx::Row;

use crate::{
    SqliteDatabase, SqliteStoreError, database_error,
    note_history::insert_note_revision,
    notes::{classify_failed_mutation, note_from_row, note_row, require_active_note_access},
};

impl SqliteDatabase {
    pub async fn read_note_acl(
        &self,
        actor: &Actor,
        note_id: NoteId,
    ) -> Result<NoteAclState, SqliteStoreError> {
        let mut transaction = self.pool.begin().await.map_err(database_error)?;
        require_active_note_access(&mut transaction, actor, note_id, NoteAccess::Manage).await?;
        let rows = sqlx::query(
            "SELECT acl.principal_id, identity.issuer, identity.subject, acl.permission
             FROM note_acl acl
             JOIN principal_identities identity
               ON identity.principal_id = acl.principal_id AND identity.is_primary = 1
             WHERE acl.note_id = ? ORDER BY identity.issuer, identity.subject",
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
                let principal_id =
                    PrincipalId::new(row.try_get("principal_id").map_err(database_error)?)
                        .map_err(|_| SqliteStoreError::CorruptData)?;
                Ok(NoteAclEntry::new(
                    PrincipalRef::new(principal_id, identity),
                    permission,
                ))
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
                             AND access.principal_id = ?
                             AND access.access_level >= 3)",
        )
        .bind(updated_at.get())
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
        sqlx::query("DELETE FROM note_acl WHERE note_id = ?")
            .bind(note_id.to_string())
            .execute(&mut *transaction)
            .await
            .map_err(database_error)?;
        for entry in entries {
            sqlx::query(
                "INSERT INTO note_acl (note_id, principal_id, permission) VALUES (?, ?, ?)",
            )
            .bind(note_id.to_string())
            .bind(entry.principal().id().get())
            .bind(match entry.permission() {
                NotePermission::Read => "read",
                NotePermission::Edit => "edit",
            })
            .execute(&mut *transaction)
            .await
            .map_err(database_error)?;
        }
        let note = note_from_row(note_row(&mut transaction, note_id).await?)?;
        insert_note_revision(
            &mut transaction,
            note_id,
            actor.principal_id(),
            NoteRevisionKind::AclUpdated,
        )
        .await?;
        transaction.commit().await.map_err(database_error)?;
        Ok(note)
    }
}
