//! 添付画像の認可済み保存と取得。

use marginalis_domain::{
    ATTACHMENT_POLICY, Actor, AttachmentId, AttachmentMediaType, AttachmentMetadata, EntityId,
    Identity, NoteId, PrincipalId, PrincipalRef, StoredAttachment, UnixMillis,
};
use sqlx::Row;

use crate::{SqliteDatabase, SqliteStoreError, database_error};

impl SqliteDatabase {
    pub async fn list_note_attachments(
        &self,
        actor: &Actor,
        note_id: NoteId,
    ) -> Result<Option<Vec<AttachmentMetadata>>, SqliteStoreError> {
        let mut transaction = self.pool.begin().await.map_err(database_error)?;
        if !note_is_readable(&mut transaction, actor, note_id).await? {
            transaction.commit().await.map_err(database_error)?;
            return Ok(None);
        }
        let rows = sqlx::query(
            "SELECT attachment.attachment_id, attachment.note_id,
                    attachment.file_name, attachment.media_type,
                    attachment.byte_length, attachment.sha256,
                    attachment.created_at_ms, attachment.created_by_principal_id,
                    identity.issuer AS created_by_issuer,
                    identity.subject AS created_by_subject
             FROM note_attachments attachment
             JOIN principal_identities identity
               ON identity.principal_id = attachment.created_by_principal_id
              AND identity.is_primary = 1
             WHERE attachment.note_id = ?
             ORDER BY attachment.created_at_ms, attachment.attachment_id",
        )
        .bind(note_id.to_string())
        .fetch_all(&mut *transaction)
        .await
        .map_err(database_error)?;
        let attachments = rows
            .into_iter()
            .map(metadata_from_row)
            .collect::<Result<Vec<_>, _>>()?;
        transaction.commit().await.map_err(database_error)?;
        Ok(Some(attachments))
    }

    pub async fn note_attachment(
        &self,
        actor: &Actor,
        note_id: NoteId,
        attachment_id: AttachmentId,
    ) -> Result<Option<StoredAttachment>, SqliteStoreError> {
        let row = sqlx::query(
            "SELECT attachment.attachment_id, attachment.note_id,
                    attachment.file_name, attachment.media_type,
                    attachment.byte_length, attachment.sha256, attachment.content,
                    attachment.created_at_ms, attachment.created_by_principal_id,
                    identity.issuer AS created_by_issuer,
                    identity.subject AS created_by_subject
             FROM note_attachments attachment
             JOIN notes ON notes.note_id = attachment.note_id
             JOIN principal_identities identity
               ON identity.principal_id = attachment.created_by_principal_id
              AND identity.is_primary = 1
             WHERE attachment.note_id = ? AND attachment.attachment_id = ?
               AND ((notes.deleted_at_ms IS NULL AND EXISTS (
                        SELECT 1 FROM note_access access
                        WHERE access.note_id = notes.note_id AND access.principal_id = ?
                    )) OR (notes.deleted_at_ms IS NOT NULL
                           AND notes.creator_principal_id = ?))",
        )
        .bind(note_id.to_string())
        .bind(attachment_id.to_string())
        .bind(actor.principal_id().get())
        .bind(actor.principal_id().get())
        .fetch_optional(&self.pool)
        .await
        .map_err(database_error)?;
        row.map(|row| {
            let bytes = row
                .try_get::<Vec<u8>, _>("content")
                .map_err(database_error)?;
            StoredAttachment::new(metadata_from_row(row)?, bytes)
                .map_err(|_| SqliteStoreError::CorruptData)
        })
        .transpose()
    }

    pub async fn create_note_attachment(
        &self,
        actor: &Actor,
        attachment: &StoredAttachment,
    ) -> Result<(), SqliteStoreError> {
        let metadata = attachment.metadata();
        if metadata.created_by().id() != actor.principal_id() {
            return Err(SqliteStoreError::CorruptData);
        }
        let mut transaction = self.pool.begin().await.map_err(database_error)?;
        if !note_is_editable(&mut transaction, actor, metadata.note_id()).await? {
            transaction.rollback().await.map_err(database_error)?;
            return Err(SqliteStoreError::NotFound);
        }
        let (count, total) = sqlx::query_as::<_, (i64, i64)>(
            "SELECT COUNT(*), COALESCE(SUM(byte_length), 0)
             FROM note_attachments WHERE note_id = ?",
        )
        .bind(metadata.note_id().to_string())
        .fetch_one(&mut *transaction)
        .await
        .map_err(database_error)?;
        let next_total = usize::try_from(total)
            .ok()
            .and_then(|total| total.checked_add(metadata.byte_length()));
        if usize::try_from(count)
            .ok()
            .is_none_or(|count| count >= ATTACHMENT_POLICY.max_attachments_per_note)
            || next_total.is_none_or(|total| total > ATTACHMENT_POLICY.max_bytes_per_note)
        {
            transaction.rollback().await.map_err(database_error)?;
            return Err(SqliteStoreError::Conflict);
        }
        sqlx::query(
            "INSERT INTO note_attachments (
                attachment_id, note_id, file_name, media_type, byte_length,
                sha256, content, created_at_ms, created_by_principal_id
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(metadata.attachment_id().to_string())
        .bind(metadata.note_id().to_string())
        .bind(metadata.file_name())
        .bind(metadata.media_type().as_str())
        .bind(i64::try_from(metadata.byte_length()).map_err(|_| SqliteStoreError::CorruptData)?)
        .bind(metadata.sha256().as_slice())
        .bind(attachment.bytes())
        .bind(metadata.created_at().get())
        .bind(metadata.created_by().id().get())
        .execute(&mut *transaction)
        .await
        .map_err(database_error)?;
        transaction.commit().await.map_err(database_error)
    }

    pub async fn delete_unused_note_attachment(
        &self,
        actor: &Actor,
        note_id: NoteId,
        attachment_id: AttachmentId,
    ) -> Result<(), SqliteStoreError> {
        let mut transaction = self.pool.begin().await.map_err(database_error)?;
        if !note_is_editable(&mut transaction, actor, note_id).await? {
            transaction.rollback().await.map_err(database_error)?;
            return Err(SqliteStoreError::NotFound);
        }
        let result = sqlx::query(
            "DELETE FROM note_attachments
             WHERE note_id = ? AND attachment_id = ?
               AND NOT EXISTS (
                    SELECT 1 FROM note_revision_attachments reference
                    WHERE reference.note_id = note_attachments.note_id
                      AND reference.attachment_id = note_attachments.attachment_id
               )",
        )
        .bind(note_id.to_string())
        .bind(attachment_id.to_string())
        .execute(&mut *transaction)
        .await
        .map_err(database_error)?;
        if result.rows_affected() != 1 {
            let exists = sqlx::query_scalar::<_, bool>(
                "SELECT EXISTS(SELECT 1 FROM note_attachments
                               WHERE note_id = ? AND attachment_id = ?)",
            )
            .bind(note_id.to_string())
            .bind(attachment_id.to_string())
            .fetch_one(&mut *transaction)
            .await
            .map_err(database_error)?;
            transaction.rollback().await.map_err(database_error)?;
            return Err(if exists {
                SqliteStoreError::Conflict
            } else {
                SqliteStoreError::NotFound
            });
        }
        transaction.commit().await.map_err(database_error)
    }
}

async fn note_is_readable(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    actor: &Actor,
    note_id: NoteId,
) -> Result<bool, SqliteStoreError> {
    sqlx::query_scalar(
        "SELECT EXISTS(
            SELECT 1 FROM notes
            WHERE note_id = ? AND (
                (deleted_at_ms IS NULL AND EXISTS (
                    SELECT 1 FROM note_access access
                    WHERE access.note_id = notes.note_id AND access.principal_id = ?
                )) OR (deleted_at_ms IS NOT NULL AND creator_principal_id = ?)
            )
        )",
    )
    .bind(note_id.to_string())
    .bind(actor.principal_id().get())
    .bind(actor.principal_id().get())
    .fetch_one(&mut **transaction)
    .await
    .map_err(database_error)
}

async fn note_is_editable(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    actor: &Actor,
    note_id: NoteId,
) -> Result<bool, SqliteStoreError> {
    sqlx::query_scalar(
        "SELECT EXISTS(
            SELECT 1 FROM notes
            JOIN note_access access ON access.note_id = notes.note_id
            WHERE notes.note_id = ? AND notes.deleted_at_ms IS NULL
              AND access.principal_id = ? AND access.access_level >= ?
        )",
    )
    .bind(note_id.to_string())
    .bind(actor.principal_id().get())
    .bind(2_i64)
    .fetch_one(&mut **transaction)
    .await
    .map_err(database_error)
}

pub(crate) fn metadata_from_row(
    row: sqlx::sqlite::SqliteRow,
) -> Result<AttachmentMetadata, SqliteStoreError> {
    let attachment_id = row
        .try_get::<String, _>("attachment_id")
        .map_err(database_error)?
        .parse::<AttachmentId>()
        .map_err(|_| SqliteStoreError::CorruptData)?;
    let note_id = row
        .try_get::<String, _>("note_id")
        .map_err(database_error)?
        .parse::<EntityId>()
        .map(NoteId::new)
        .map_err(|_| SqliteStoreError::CorruptData)?;
    let sha256 = row
        .try_get::<Vec<u8>, _>("sha256")
        .map_err(database_error)?
        .try_into()
        .map_err(|_| SqliteStoreError::CorruptData)?;
    let identity = Identity::new(
        row.try_get("created_by_issuer").map_err(database_error)?,
        row.try_get("created_by_subject").map_err(database_error)?,
    )
    .map_err(|_| SqliteStoreError::CorruptData)?;
    let principal = PrincipalRef::new(
        PrincipalId::new(
            row.try_get("created_by_principal_id")
                .map_err(database_error)?,
        )
        .map_err(|_| SqliteStoreError::CorruptData)?,
        identity,
    );
    AttachmentMetadata::new(
        attachment_id,
        note_id,
        row.try_get("file_name").map_err(database_error)?,
        row.try_get::<String, _>("media_type")
            .map_err(database_error)?
            .parse::<AttachmentMediaType>()
            .map_err(|_| SqliteStoreError::CorruptData)?,
        usize::try_from(
            row.try_get::<i64, _>("byte_length")
                .map_err(database_error)?,
        )
        .map_err(|_| SqliteStoreError::CorruptData)?,
        sha256,
        UnixMillis::new(row.try_get("created_at_ms").map_err(database_error)?),
        principal,
    )
    .map_err(|_| SqliteStoreError::CorruptData)
}
