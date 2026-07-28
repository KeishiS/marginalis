//! 全ノートの可搬archive import/export。

use std::collections::HashSet;

use marginalis_domain::{
    ArchivedNoteAclEntry, EntityId, Note, NoteId, NotePermission, validate_identity,
};
use sqlx::Sqlite;

use crate::{SqliteDatabase, SqliteStoreError, database_error, notes::note_from_row};

impl SqliteDatabase {
    /// SQLite正本のノートとACLを同じ読み取りtransactionから取り出す。
    pub async fn export_archive_snapshot(
        &self,
    ) -> Result<(Vec<Note>, Vec<ArchivedNoteAclEntry>), SqliteStoreError> {
        let mut transaction = self.pool.begin().await.map_err(database_error)?;
        let rows = sqlx::query(
            "SELECT note_id, creator_issuer, creator_subject, title, body, tags_json, created_at_ms, updated_at_ms, revision, deleted_at_ms
             FROM notes ORDER BY note_id ASC",
        )
        .fetch_all(&mut *transaction)
        .await
        .map_err(database_error)?;
        let notes = rows
            .into_iter()
            .map(note_from_row)
            .collect::<Result<Vec<_>, _>>()?;
        let rows = sqlx::query(
            "SELECT note_id, subject, permission FROM note_acl ORDER BY note_id, subject",
        )
        .fetch_all(&mut *transaction)
        .await
        .map_err(database_error)?;
        let note_acl = rows
            .into_iter()
            .map(|row| {
                use sqlx::Row;
                let note_id = row
                    .try_get::<String, _>("note_id")
                    .map_err(database_error)?
                    .parse::<EntityId>()
                    .map(NoteId::new)
                    .map_err(|_| SqliteStoreError::CorruptData)?;
                let permission = match row
                    .try_get::<String, _>("permission")
                    .map_err(database_error)?
                    .as_str()
                {
                    "read" => NotePermission::Read,
                    "edit" => NotePermission::Edit,
                    _ => return Err(SqliteStoreError::CorruptData),
                };
                Ok(ArchivedNoteAclEntry {
                    note_id,
                    subject: row.try_get("subject").map_err(database_error)?,
                    permission,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        transaction.commit().await.map_err(database_error)?;
        Ok((notes, note_acl))
    }

    /// 検証済みの論理snapshotを空databaseへ一つのtransactionでimportする。
    pub async fn import_notes(
        &self,
        notes: &[Note],
        references: &[(NoteId, NoteId)],
        note_acl: &[ArchivedNoteAclEntry],
    ) -> Result<(), SqliteStoreError> {
        let mut note_ids = HashSet::new();
        for note in notes {
            if !note_ids.insert(note.note_id()) {
                return Err(SqliteStoreError::CorruptData);
            }
        }

        let mut transaction = self.pool.begin().await.map_err(database_error)?;
        let target_has_data = sqlx::query_scalar::<_, bool>(
            "SELECT
                EXISTS(SELECT 1 FROM notes)
                OR EXISTS(SELECT 1 FROM web_sessions)
                OR EXISTS(SELECT 1 FROM oidc_login_attempts)
                OR EXISTS(SELECT 1 FROM mcp_clients)
                OR EXISTS(SELECT 1 FROM mcp_authorization_codes)
                OR EXISTS(SELECT 1 FROM mcp_access_tokens)
                OR EXISTS(SELECT 1 FROM mcp_refresh_tokens)",
        )
        .fetch_one(&mut *transaction)
        .await
        .map_err(database_error)?;
        if target_has_data {
            return Err(SqliteStoreError::ArchiveTargetNotEmpty);
        }
        for note in notes {
            insert_note_row(&mut transaction, note).await?;
        }
        for (source, target) in references {
            if !note_ids.contains(source) {
                return Err(SqliteStoreError::CorruptData);
            }
            sqlx::query(
                "INSERT OR IGNORE INTO note_references (source_note_id, target_note_id) VALUES (?, ?)",
            )
            .bind(source.to_string())
            .bind(target.to_string())
            .execute(&mut *transaction)
            .await
            .map_err(database_error)?;
        }
        for entry in note_acl {
            let Some(note) = notes.iter().find(|note| note.note_id() == entry.note_id) else {
                return Err(SqliteStoreError::CorruptData);
            };
            if validate_identity(note.creator_issuer(), &entry.subject).is_err()
                || entry.subject == note.creator_subject()
            {
                return Err(SqliteStoreError::CorruptData);
            }
            sqlx::query("INSERT INTO note_acl (note_id, subject, permission) VALUES (?, ?, ?)")
                .bind(entry.note_id.to_string())
                .bind(&entry.subject)
                .bind(match entry.permission {
                    NotePermission::Read => "read",
                    NotePermission::Edit => "edit",
                })
                .execute(&mut *transaction)
                .await
                .map_err(database_error)?;
        }
        transaction.commit().await.map_err(database_error)
    }
}

async fn insert_note_row(
    transaction: &mut sqlx::Transaction<'_, Sqlite>,
    note: &Note,
) -> Result<(), SqliteStoreError> {
    let tags_json =
        serde_json::to_string(note.tags()).map_err(|_| SqliteStoreError::CorruptData)?;
    sqlx::query(
        "INSERT INTO notes (note_id, creator_issuer, creator_subject, title, body, tags_json, created_at_ms, updated_at_ms, revision, deleted_at_ms)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(note.note_id().to_string())
    .bind(note.creator_issuer())
    .bind(note.creator_subject())
    .bind(note.title())
    .bind(note.body())
    .bind(tags_json)
    .bind(note.created_at().get())
    .bind(note.updated_at().get())
    .bind(note.revision())
    .bind(note.deleted_at().map(marginalis_domain::UnixMillis::get))
    .execute(&mut **transaction)
    .await
    .map_err(database_error)?;
    Ok(())
}
