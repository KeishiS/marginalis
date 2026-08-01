//! 全ノートの可搬archive import/export。

use marginalis_application::{LogicalSnapshot, NoteAclSnapshotEntry, RestorePlan};
use marginalis_domain::{BibliographyItem, EntityId, Identity, Note, NoteId, NotePermission};
use sqlx::Sqlite;

use crate::{SqliteDatabase, SqliteStoreError, database_error, notes::note_from_row};

impl SqliteDatabase {
    /// SQLite正本のノートとACLを同じ読み取りtransactionから取り出す。
    pub async fn export_archive_snapshot(&self) -> Result<LogicalSnapshot, SqliteStoreError> {
        let mut transaction = self.pool.begin().await.map_err(database_error)?;
        let rows = sqlx::query(
            "SELECT note_id, creator_issuer, creator_subject, title, source, tags_json, created_at_ms, updated_at_ms, revision, deleted_at_ms
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
            "SELECT note_id, issuer, subject, permission
             FROM note_acl ORDER BY note_id, issuer, subject",
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
                Ok(NoteAclSnapshotEntry::new(
                    note_id,
                    Identity::new(
                        row.try_get("issuer").map_err(database_error)?,
                        row.try_get("subject").map_err(database_error)?,
                    )
                    .map_err(|_| SqliteStoreError::CorruptData)?,
                    permission,
                ))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let rows = sqlx::query(
            "SELECT item_id, owner_issuer, owner_subject, citation_key, csl_json,
                    created_at_ms, updated_at_ms, revision
             FROM bibliography_items ORDER BY item_id",
        )
        .fetch_all(&mut *transaction)
        .await
        .map_err(database_error)?;
        let bibliography_items = rows
            .into_iter()
            .map(crate::bibliography_repository::decode_item)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| SqliteStoreError::CorruptData)?;
        transaction.commit().await.map_err(database_error)?;
        LogicalSnapshot::new(notes, note_acl)
            .and_then(|snapshot| snapshot.with_bibliography(bibliography_items))
            .map_err(|_| SqliteStoreError::CorruptData)
    }

    /// 検証済みの復元計画を空databaseへ一つのtransactionで適用する。
    pub async fn restore(&self, plan: &RestorePlan) -> Result<(), SqliteStoreError> {
        let notes = plan.snapshot().notes();

        let mut transaction = self.pool.begin().await.map_err(database_error)?;
        let target_has_data = sqlx::query_scalar::<_, bool>(
            "SELECT
                EXISTS(SELECT 1 FROM notes)
                OR EXISTS(SELECT 1 FROM bibliography_items)
                OR EXISTS(SELECT 1 FROM web_sessions)
                OR EXISTS(SELECT 1 FROM oidc_login_attempts)",
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
        for item in plan.snapshot().bibliography_items() {
            insert_bibliography_item_row(&mut transaction, item).await?;
        }
        for (source, key) in plan.citations() {
            sqlx::query(
                "INSERT OR IGNORE INTO note_citations (source_note_id, citation_key) VALUES (?, ?)",
            )
            .bind(source.to_string())
            .bind(key)
            .execute(&mut *transaction)
            .await
            .map_err(database_error)?;
        }
        for (source, target) in plan.references() {
            sqlx::query(
                "INSERT OR IGNORE INTO note_references (source_note_id, target_note_id) VALUES (?, ?)",
            )
            .bind(source.to_string())
            .bind(target.to_string())
            .execute(&mut *transaction)
            .await
            .map_err(database_error)?;
        }
        for entry in plan.snapshot().note_acl() {
            sqlx::query(
                "INSERT INTO note_acl (note_id, issuer, subject, permission) VALUES (?, ?, ?, ?)",
            )
            .bind(entry.note_id().to_string())
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
        transaction.commit().await.map_err(database_error)
    }
}

async fn insert_bibliography_item_row(
    transaction: &mut sqlx::Transaction<'_, Sqlite>,
    item: &BibliographyItem,
) -> Result<(), SqliteStoreError> {
    sqlx::query(
        "INSERT INTO bibliography_items (
            item_id, owner_issuer, owner_subject, citation_key, csl_json,
            created_at_ms, updated_at_ms, revision
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(item.item_id().to_string())
    .bind(item.owner().issuer())
    .bind(item.owner().subject())
    .bind(item.citation_key())
    .bind(item.csl_json())
    .bind(item.created_at().get())
    .bind(item.updated_at().get())
    .bind(item.revision().get())
    .execute(&mut **transaction)
    .await
    .map_err(database_error)?;
    Ok(())
}

async fn insert_note_row(
    transaction: &mut sqlx::Transaction<'_, Sqlite>,
    note: &Note,
) -> Result<(), SqliteStoreError> {
    let tags_json =
        serde_json::to_string(note.tags()).map_err(|_| SqliteStoreError::CorruptData)?;
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
    .bind(note.deleted_at().map(marginalis_domain::UnixMillis::get))
    .execute(&mut **transaction)
    .await
    .map_err(database_error)?;
    Ok(())
}
