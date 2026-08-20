//! 全ノートの可搬archive import/export。

use marginalis_application::{
    LogicalSnapshot, MathMacroSettingsSnapshot, NoteAclSnapshotEntry, RestorePlan,
};
use marginalis_domain::{
    BibliographyItem, EntityId, Identity, Note, NoteId, NotePermission, PrincipalId, PrincipalRef,
};
use sqlx::Sqlite;

use crate::{SqliteDatabase, SqliteStoreError, database_error, notes::note_from_row};

impl SqliteDatabase {
    /// SQLite正本のノートとACLを同じ読み取りtransactionから取り出す。
    pub async fn export_archive_snapshot(&self) -> Result<LogicalSnapshot, SqliteStoreError> {
        let mut transaction = self.pool.begin().await.map_err(database_error)?;
        let rows = sqlx::query("SELECT * FROM note_details ORDER BY note_id ASC")
            .fetch_all(&mut *transaction)
            .await
            .map_err(database_error)?;
        let notes = rows
            .into_iter()
            .map(note_from_row)
            .collect::<Result<Vec<_>, _>>()?;
        let rows = sqlx::query(
            "SELECT acl.note_id, acl.principal_id, identity.issuer, identity.subject,
                    acl.permission
             FROM note_acl acl
             JOIN principal_identities identity
               ON identity.principal_id = acl.principal_id AND identity.is_primary = 1
             ORDER BY acl.note_id, identity.issuer, identity.subject",
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
                    PrincipalRef::new(
                        PrincipalId::new(row.try_get("principal_id").map_err(database_error)?)
                            .map_err(|_| SqliteStoreError::CorruptData)?,
                        Identity::new(
                            row.try_get("issuer").map_err(database_error)?,
                            row.try_get("subject").map_err(database_error)?,
                        )
                        .map_err(|_| SqliteStoreError::CorruptData)?,
                    ),
                    permission,
                ))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let rows = sqlx::query(
            "SELECT item_id, owner_principal_id, owner_issuer, owner_subject, citation_key, csl_json,
                    created_at_ms, updated_at_ms, revision
             FROM bibliography_item_details ORDER BY item_id",
        )
        .fetch_all(&mut *transaction)
        .await
        .map_err(database_error)?;
        let bibliography_items = rows
            .into_iter()
            .map(crate::bibliography_repository::decode_item)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| SqliteStoreError::CorruptData)?;
        let rows = sqlx::query(
            "SELECT source_id, owner_principal_id, owner_issuer, owner_subject, method, display_name, revision,
                    created_at_ms, last_imported_at_ms
             FROM bibliography_import_source_details ORDER BY source_id",
        )
        .fetch_all(&mut *transaction)
        .await
        .map_err(database_error)?;
        let bibliography_import_sources = rows
            .into_iter()
            .map(crate::bibliography_import_repository::decode_source)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| SqliteStoreError::CorruptData)?;
        let rows = sqlx::query(
            "SELECT source_id, external_item_id, item_id, imported_digest,
                    imported_item_revision
             FROM bibliography_import_links ORDER BY source_id, external_item_id",
        )
        .fetch_all(&mut *transaction)
        .await
        .map_err(database_error)?;
        let bibliography_import_links = rows
            .into_iter()
            .map(crate::bibliography_import_repository::decode_link)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| SqliteStoreError::CorruptData)?;
        let rows = sqlx::query(
            "SELECT settings.owner_principal_id, identity.issuer AS owner_issuer,
                    identity.subject AS owner_subject, settings.macros_json, settings.revision
             FROM math_macro_settings settings
             JOIN principal_identities identity
               ON identity.principal_id = settings.owner_principal_id AND identity.is_primary = 1
             ORDER BY identity.issuer, identity.subject",
        )
        .fetch_all(&mut *transaction)
        .await
        .map_err(database_error)?;
        let math_macro_settings = rows
            .into_iter()
            .map(|row| {
                use sqlx::Row;
                let identity = Identity::new(
                    row.try_get("owner_issuer").map_err(database_error)?,
                    row.try_get("owner_subject").map_err(database_error)?,
                )
                .map_err(|_| SqliteStoreError::CorruptData)?;
                let owner = PrincipalRef::new(
                    PrincipalId::new(row.try_get("owner_principal_id").map_err(database_error)?)
                        .map_err(|_| SqliteStoreError::CorruptData)?,
                    identity,
                );
                let settings = crate::math_macro_repository::decode_settings(row)
                    .map_err(|_| SqliteStoreError::CorruptData)?;
                Ok(MathMacroSettingsSnapshot::new(owner, settings))
            })
            .collect::<Result<Vec<_>, SqliteStoreError>>()?;
        transaction.commit().await.map_err(database_error)?;
        LogicalSnapshot::new(notes, note_acl)
            .and_then(|snapshot| {
                snapshot.with_bibliography_data(
                    bibliography_items,
                    bibliography_import_sources,
                    bibliography_import_links,
                )
            })
            .and_then(|snapshot| snapshot.with_math_macro_settings(math_macro_settings))
            .map_err(|_| SqliteStoreError::CorruptData)
    }

    /// 検証済みの復元計画を空databaseへ一つのtransactionで適用する。
    pub async fn restore(&self, plan: &RestorePlan) -> Result<(), SqliteStoreError> {
        let notes = plan.snapshot().notes();

        let mut transaction = self.pool.begin().await.map_err(database_error)?;
        let target_has_data = sqlx::query_scalar::<_, bool>(
            "SELECT
                EXISTS(SELECT 1 FROM principals)
                OR EXISTS(SELECT 1 FROM notes)
                OR EXISTS(SELECT 1 FROM bibliography_items)
                OR EXISTS(SELECT 1 FROM bibliography_import_sources)
                OR EXISTS(SELECT 1 FROM bibliography_import_links)
                OR EXISTS(SELECT 1 FROM math_macro_settings)
                OR EXISTS(SELECT 1 FROM web_sessions)
                OR EXISTS(SELECT 1 FROM oidc_login_attempts)
                OR EXISTS(SELECT 1 FROM mcp_clients)
                OR EXISTS(SELECT 1 FROM mcp_client_authorizations)
                OR EXISTS(SELECT 1 FROM mcp_principal_scope_ceilings)
                OR EXISTS(SELECT 1 FROM mcp_client_scope_ceilings)
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
            ensure_principal(&mut transaction, note.owner()).await?;
            if let Some(review) = note.last_review() {
                ensure_principal(&mut transaction, review.reviewer()).await?;
            }
        }
        for item in plan.snapshot().bibliography_items() {
            ensure_principal(&mut transaction, item.owner()).await?;
        }
        for source in plan.snapshot().bibliography_import_sources() {
            ensure_principal(&mut transaction, source.owner()).await?;
        }
        for entry in plan.snapshot().math_macro_settings() {
            ensure_principal(&mut transaction, entry.owner()).await?;
        }
        for entry in plan.snapshot().note_acl() {
            ensure_principal(&mut transaction, entry.principal()).await?;
        }
        for note in notes {
            insert_note_row(&mut transaction, note).await?;
        }
        for item in plan.snapshot().bibliography_items() {
            insert_bibliography_item_row(&mut transaction, item).await?;
        }
        for source in plan.snapshot().bibliography_import_sources() {
            insert_bibliography_import_source_row(&mut transaction, source).await?;
        }
        for link in plan.snapshot().bibliography_import_links() {
            insert_bibliography_import_link_row(&mut transaction, link).await?;
        }
        for entry in plan.snapshot().math_macro_settings() {
            let encoded = serde_json::to_string(
                &entry
                    .settings()
                    .macros
                    .iter()
                    .map(|item| crate::math_macro_repository::StoredMathMacro {
                        name: item.name.clone(),
                        replacement: item.replacement.clone(),
                        argument_count: item.argument_count,
                    })
                    .collect::<Vec<_>>(),
            )
            .map_err(|_| SqliteStoreError::CorruptData)?;
            sqlx::query(
                "INSERT INTO math_macro_settings (
                    owner_principal_id, macros_json, revision
                 ) VALUES (?, ?, ?)",
            )
            .bind(entry.owner().id().get())
            .bind(encoded)
            .bind(entry.settings().revision)
            .execute(&mut *transaction)
            .await
            .map_err(database_error)?;
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
                "INSERT INTO note_acl (note_id, principal_id, permission) VALUES (?, ?, ?)",
            )
            .bind(entry.note_id().to_string())
            .bind(entry.principal().id().get())
            .bind(match entry.permission() {
                NotePermission::Read => "read",
                NotePermission::Edit => "edit",
            })
            .execute(&mut *transaction)
            .await
            .map_err(database_error)?;
        }
        // 同期cursorと変更索引は外部投影の一時状態であり、archiveへ含めない。
        // 復元中にtriggerが作った変更も捨て、外部clientには新しい全量同期を要求する。
        sqlx::query("DELETE FROM note_sync_cursors")
            .execute(&mut *transaction)
            .await
            .map_err(database_error)?;
        sqlx::query("DELETE FROM note_sync_changes")
            .execute(&mut *transaction)
            .await
            .map_err(database_error)?;
        sqlx::query("UPDATE note_sync_state SET next_sequence = 0 WHERE singleton = 1")
            .execute(&mut *transaction)
            .await
            .map_err(database_error)?;
        transaction.commit().await.map_err(database_error)
    }
}

async fn insert_bibliography_import_source_row(
    transaction: &mut sqlx::Transaction<'_, Sqlite>,
    source: &marginalis_domain::BibliographyImportSource,
) -> Result<(), SqliteStoreError> {
    sqlx::query(
        "INSERT INTO bibliography_import_sources (
            source_id, owner_principal_id, method, display_name, revision,
            created_at_ms, last_imported_at_ms
         ) VALUES (?, ?, 'csl_json_file', ?, ?, ?, ?)",
    )
    .bind(source.source_id().to_string())
    .bind(source.owner().id().get())
    .bind(source.display_name())
    .bind(source.revision().get())
    .bind(source.created_at().get())
    .bind(source.last_imported_at().get())
    .execute(&mut **transaction)
    .await
    .map_err(database_error)?;
    Ok(())
}

async fn insert_bibliography_import_link_row(
    transaction: &mut sqlx::Transaction<'_, Sqlite>,
    link: &marginalis_domain::BibliographyImportLink,
) -> Result<(), SqliteStoreError> {
    sqlx::query(
        "INSERT INTO bibliography_import_links (
            source_id, external_item_id, item_id, owner_principal_id,
            imported_digest, imported_item_revision
         )
         SELECT ?, ?, ?, source.owner_principal_id, ?, ?
         FROM bibliography_import_sources source
         WHERE source.source_id = ?",
    )
    .bind(link.source_id().to_string())
    .bind(link.external_item_id())
    .bind(link.item_id().to_string())
    .bind(link.imported_digest().as_bytes().as_slice())
    .bind(link.imported_item_revision().get())
    .bind(link.source_id().to_string())
    .execute(&mut **transaction)
    .await
    .map_err(database_error)?;
    Ok(())
}

async fn insert_bibliography_item_row(
    transaction: &mut sqlx::Transaction<'_, Sqlite>,
    item: &BibliographyItem,
) -> Result<(), SqliteStoreError> {
    sqlx::query(
        "INSERT INTO bibliography_items (
            item_id, owner_principal_id, citation_key, csl_json,
            created_at_ms, updated_at_ms, revision
         ) VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(item.item_id().to_string())
    .bind(item.owner().id().get())
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
        "INSERT INTO notes (
            note_id, creator_principal_id, title, source, tags_json,
            created_at_ms, updated_at_ms, revision, deleted_at_ms, created_via,
            review_tracking_known, reviewed_revision, reviewed_at_ms,
            reviewer_principal_id
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(note.note_id().to_string())
    .bind(note.owner().id().get())
    .bind(note.title())
    .bind(note.source())
    .bind(tags_json)
    .bind(note.created_at().get())
    .bind(note.updated_at().get())
    .bind(note.revision().get())
    .bind(note.deleted_at().map(marginalis_domain::UnixMillis::get))
    .bind(note.created_via().as_str())
    .bind(i64::from(note.review_tracking_known()))
    .bind(note.last_review().map(|review| review.revision().get()))
    .bind(note.last_review().map(|review| review.reviewed_at().get()))
    .bind(
        note.last_review()
            .map(|review| review.reviewer().id().get()),
    )
    .execute(&mut **transaction)
    .await
    .map_err(database_error)?;
    Ok(())
}

async fn ensure_principal(
    transaction: &mut sqlx::Transaction<'_, Sqlite>,
    principal: &PrincipalRef,
) -> Result<(), SqliteStoreError> {
    sqlx::query("INSERT OR IGNORE INTO principals (principal_id) VALUES (?)")
        .bind(principal.id().get())
        .execute(&mut **transaction)
        .await
        .map_err(database_error)?;
    let identity = principal.primary_identity();
    sqlx::query(
        "INSERT OR IGNORE INTO principal_identities
             (principal_id, issuer, subject, is_primary)
         VALUES (?, ?, ?, 1)",
    )
    .bind(principal.id().get())
    .bind(identity.issuer())
    .bind(identity.subject())
    .execute(&mut **transaction)
    .await
    .map_err(database_error)?;
    let valid = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(
             SELECT 1 FROM principal_identities
             WHERE principal_id = ? AND issuer = ? AND subject = ? AND is_primary = 1
         )",
    )
    .bind(principal.id().get())
    .bind(identity.issuer())
    .bind(identity.subject())
    .fetch_one(&mut **transaction)
    .await
    .map_err(database_error)?;
    if !valid {
        return Err(SqliteStoreError::CorruptData);
    }
    Ok(())
}
