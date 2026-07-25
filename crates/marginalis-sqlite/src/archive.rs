//! 全ノートとACLの可搬archive import/export。

use std::{
    collections::{HashMap, HashSet},
    str::FromStr,
};

use marginalis_domain::{
    ARCHIVE_FORMAT, Archive, EntityId, Note, NoteAclEntry, NoteBundle, NoteId, NotePermission,
};
use sqlx::{Row, Sqlite};

use crate::{
    SqliteDatabase, SqliteStoreError, database_error,
    notes::{note_from_row, permission_from_storage, permission_to_storage},
};

impl SqliteDatabase {
    /// SQLite正本を可搬 archive の論理表現として取り出す。
    pub async fn export_archive(&self) -> Result<Archive, SqliteStoreError> {
        let mut transaction = self.pool.begin().await.map_err(database_error)?;
        let rows = sqlx::query(
            "SELECT note_id, creator_issuer, creator_subject, title, body, tags_json, created_at_ms, updated_at_ms, revision, deleted_at_ms
             FROM notes ORDER BY note_id ASC",
        )
        .fetch_all(&mut *transaction)
        .await
        .map_err(database_error)?;
        let acl_rows = sqlx::query(
            "SELECT note_id, issuer, subject, permission
             FROM note_acl ORDER BY note_id ASC, issuer ASC, subject ASC",
        )
        .fetch_all(&mut *transaction)
        .await
        .map_err(database_error)?;
        let mut acl_by_note = HashMap::<NoteId, Vec<NoteAclEntry>>::new();
        for row in acl_rows {
            let note_id = row
                .try_get::<String, _>("note_id")
                .map_err(database_error)?
                .parse::<EntityId>()
                .map(NoteId::new)
                .map_err(|_| SqliteStoreError::CorruptData)?;
            let permission = permission_from_storage(
                row.try_get::<i64, _>("permission")
                    .map_err(database_error)?,
            )?;
            acl_by_note.entry(note_id).or_default().push(NoteAclEntry {
                issuer: row.try_get("issuer").map_err(database_error)?,
                subject: row.try_get("subject").map_err(database_error)?,
                permission,
            });
        }
        let mut notes = Vec::with_capacity(rows.len());
        for row in rows {
            let note = note_from_row(row)?;
            let acl = acl_by_note.remove(&note.note_id).unwrap_or_default();
            notes.push(NoteBundle { note, acl });
        }
        transaction.commit().await.map_err(database_error)?;
        Ok(Archive {
            format: ARCHIVE_FORMAT.into(),
            notes,
        })
    }

    /// 検証済みarchiveを空の v0.3.0 databaseへ一つのtransactionでimportする。
    pub async fn import_archive(&self, archive: &Archive) -> Result<(), SqliteStoreError> {
        if archive.format != ARCHIVE_FORMAT {
            return Err(SqliteStoreError::ArchiveFormat);
        }
        let mut note_ids = HashSet::new();
        for bundle in &archive.notes {
            if !note_ids.insert(bundle.note.note_id) {
                return Err(SqliteStoreError::CorruptData);
            }
            if EntityId::from_str(&bundle.note.note_id.to_string()).is_err()
                || bundle.note.creator_issuer.trim().is_empty()
                || bundle.note.creator_subject.trim().is_empty()
                || bundle.note.created_at > bundle.note.updated_at
                || bundle
                    .note
                    .deleted_at
                    .is_some_and(|deleted_at| deleted_at < bundle.note.created_at)
                || bundle.note.revision <= 0
            {
                return Err(SqliteStoreError::CorruptData);
            }
            let mut acl_subjects = HashSet::new();
            for entry in &bundle.acl {
                if entry.issuer.trim().is_empty()
                    || entry.subject.trim().is_empty()
                    || !acl_subjects.insert((&entry.issuer, &entry.subject))
                {
                    return Err(SqliteStoreError::CorruptData);
                }
            }
            if !bundle
                .acl
                .iter()
                .any(|entry| entry.permission == NotePermission::Admin)
            {
                return Err(SqliteStoreError::ArchiveMissingAdmin);
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
        for bundle in &archive.notes {
            insert_note_row(&mut transaction, &bundle.note).await?;
            for entry in &bundle.acl {
                sqlx::query(
                    "INSERT INTO note_acl (note_id, issuer, subject, permission) VALUES (?, ?, ?, ?)",
                )
                .bind(bundle.note.note_id.to_string())
                .bind(&entry.issuer)
                .bind(&entry.subject)
                .bind(permission_to_storage(entry.permission))
                .execute(&mut *transaction)
                .await
                .map_err(database_error)?;
            }
        }
        transaction.commit().await.map_err(database_error)
    }
}

async fn insert_note_row(
    transaction: &mut sqlx::Transaction<'_, Sqlite>,
    note: &Note,
) -> Result<(), SqliteStoreError> {
    let tags_json = serde_json::to_string(&note.tags).map_err(|_| SqliteStoreError::CorruptData)?;
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
    .bind(note.deleted_at.map(marginalis_domain::UnixMillis::get))
    .execute(&mut **transaction)
    .await
    .map_err(database_error)?;
    Ok(())
}
