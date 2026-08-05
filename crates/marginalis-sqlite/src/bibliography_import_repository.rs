//! CSL-JSON一方向取り込みのSQLite実装。

use std::str::FromStr;

use async_trait::async_trait;
use marginalis_application::{
    BibliographyImportCommit, BibliographyImportItemMutation, BibliographyImportRepository,
    BibliographyImportRepositoryError, BibliographyImportResult, BibliographyImportState,
};
use marginalis_domain::{
    Actor, BibliographyContentDigest, BibliographyImportLink, BibliographyImportMethod,
    BibliographyImportSource, BibliographyImportSourceId, BibliographyItemId, EntityId, Identity,
    Revision, UnixMillis,
};
use sqlx::{Row, Sqlite, Transaction};

use crate::{SqliteDatabase, bibliography_repository::decode_item, database_error};

#[async_trait]
impl BibliographyImportRepository for SqliteDatabase {
    async fn list_import_sources(
        &self,
        actor: &Actor,
    ) -> Result<Vec<BibliographyImportSource>, BibliographyImportRepositoryError> {
        let rows = sqlx::query(
            "SELECT source_id, owner_issuer, owner_subject, method, display_name, revision,
                    created_at_ms, last_imported_at_ms
             FROM bibliography_import_sources
             WHERE owner_issuer = ? AND owner_subject = ?
             ORDER BY last_imported_at_ms DESC, source_id",
        )
        .bind(actor.issuer())
        .bind(actor.subject())
        .fetch_all(&self.pool)
        .await
        .map_err(map_database_error)?;
        rows.into_iter().map(decode_source).collect()
    }

    async fn load_import_state(
        &self,
        actor: &Actor,
        source_id: Option<BibliographyImportSourceId>,
    ) -> Result<BibliographyImportState, BibliographyImportRepositoryError> {
        let mut transaction = self.pool.begin().await.map_err(map_database_error)?;
        let state = load_state_in_transaction(&mut transaction, actor, source_id).await?;
        transaction.commit().await.map_err(map_database_error)?;
        Ok(state)
    }

    async fn apply_import(
        &self,
        actor: &Actor,
        commit: BibliographyImportCommit,
    ) -> Result<BibliographyImportResult, BibliographyImportRepositoryError> {
        if commit.source.owner() != actor.identity() {
            return Err(BibliographyImportRepositoryError::NotFound);
        }
        if !commit_is_consistent(&commit) {
            return Err(BibliographyImportRepositoryError::Conflict);
        }
        let mut transaction = self.pool.begin().await.map_err(map_database_error)?;
        let current_state = load_state_in_transaction(
            &mut transaction,
            actor,
            commit
                .expected_state
                .source
                .as_ref()
                .map(|source| source.source_id()),
        )
        .await?;
        if current_state != commit.expected_state {
            return Err(BibliographyImportRepositoryError::Conflict);
        }
        let source_revision = persist_source(&mut transaction, actor, &commit).await?;
        let mut created = 0;
        let mut updated = 0;
        let mut kept = 0;
        for mutation in &commit.mutations {
            match mutation {
                BibliographyImportItemMutation::Create { item, link } => {
                    if item.owner() != actor.identity() {
                        return Err(BibliographyImportRepositoryError::NotFound);
                    }
                    sqlx::query(
                        "INSERT INTO bibliography_items (
                            item_id, owner_issuer, owner_subject, citation_key, csl_json,
                            created_at_ms, updated_at_ms, revision
                         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
                    )
                    .bind(item.item_id().to_string())
                    .bind(actor.issuer())
                    .bind(actor.subject())
                    .bind(item.citation_key())
                    .bind(item.csl_json())
                    .bind(item.created_at().get())
                    .bind(item.updated_at().get())
                    .bind(item.revision().get())
                    .execute(&mut *transaction)
                    .await
                    .map_err(map_write_error)?;
                    persist_link(&mut transaction, actor, link).await?;
                    created += 1;
                }
                BibliographyImportItemMutation::Update {
                    item_id,
                    csl_json,
                    expected_revision,
                    link,
                    updated_at,
                } => {
                    let result = sqlx::query(
                        "UPDATE bibliography_items
                         SET csl_json = ?, updated_at_ms = ?, revision = revision + 1
                         WHERE item_id = ? AND owner_issuer = ? AND owner_subject = ?
                           AND revision = ?",
                    )
                    .bind(csl_json)
                    .bind(updated_at.get())
                    .bind(item_id.to_string())
                    .bind(actor.issuer())
                    .bind(actor.subject())
                    .bind(expected_revision.get())
                    .execute(&mut *transaction)
                    .await
                    .map_err(map_write_error)?;
                    if result.rows_affected() != 1 {
                        return Err(BibliographyImportRepositoryError::Conflict);
                    }
                    persist_link(&mut transaction, actor, link).await?;
                    updated += 1;
                }
                BibliographyImportItemMutation::Keep {
                    expected_revision,
                    link,
                } => {
                    let exists = sqlx::query_scalar::<_, i64>(
                        "SELECT COUNT(*) FROM bibliography_items
                         WHERE item_id = ? AND owner_issuer = ? AND owner_subject = ?
                           AND revision = ?",
                    )
                    .bind(link.item_id().to_string())
                    .bind(actor.issuer())
                    .bind(actor.subject())
                    .bind(expected_revision.get())
                    .fetch_one(&mut *transaction)
                    .await
                    .map_err(map_database_error)?
                        == 1;
                    if !exists {
                        return Err(BibliographyImportRepositoryError::Conflict);
                    }
                    persist_link(&mut transaction, actor, link).await?;
                    kept += 1;
                }
            }
        }
        transaction.commit().await.map_err(map_database_error)?;
        Ok(BibliographyImportResult {
            source_id: commit.source.source_id(),
            source_revision,
            created,
            updated,
            kept,
            excluded: commit.excluded,
        })
    }
}

fn commit_is_consistent(commit: &BibliographyImportCommit) -> bool {
    let source_id = commit.source.source_id();
    commit.mutations.iter().all(|mutation| match mutation {
        BibliographyImportItemMutation::Create { item, link } => {
            link.source_id() == source_id
                && link.item_id() == item.item_id()
                && link.imported_item_revision() == item.revision()
        }
        BibliographyImportItemMutation::Update {
            item_id,
            expected_revision,
            link,
            ..
        } => {
            link.source_id() == source_id
                && link.item_id() == *item_id
                && expected_revision
                    .get()
                    .checked_add(1)
                    .is_some_and(|revision| revision == link.imported_item_revision().get())
        }
        BibliographyImportItemMutation::Keep {
            expected_revision,
            link,
        } => link.source_id() == source_id && link.imported_item_revision() == *expected_revision,
    })
}

async fn load_state_in_transaction(
    transaction: &mut Transaction<'_, Sqlite>,
    actor: &Actor,
    source_id: Option<BibliographyImportSourceId>,
) -> Result<BibliographyImportState, BibliographyImportRepositoryError> {
    let source = if let Some(source_id) = source_id {
        sqlx::query(
            "SELECT source_id, owner_issuer, owner_subject, method, display_name, revision,
                    created_at_ms, last_imported_at_ms
             FROM bibliography_import_sources
             WHERE source_id = ? AND owner_issuer = ? AND owner_subject = ?",
        )
        .bind(source_id.to_string())
        .bind(actor.issuer())
        .bind(actor.subject())
        .fetch_optional(&mut **transaction)
        .await
        .map_err(map_database_error)?
        .map(decode_source)
        .transpose()?
    } else {
        None
    };
    let links = if let Some(source_id) = source_id {
        let rows = sqlx::query(
            "SELECT source_id, external_item_id, item_id, imported_digest,
                    imported_item_revision
             FROM bibliography_import_links
             WHERE source_id = ? AND owner_issuer = ? AND owner_subject = ?
             ORDER BY external_item_id",
        )
        .bind(source_id.to_string())
        .bind(actor.issuer())
        .bind(actor.subject())
        .fetch_all(&mut **transaction)
        .await
        .map_err(map_database_error)?;
        rows.into_iter()
            .map(decode_link)
            .collect::<Result<_, _>>()?
    } else {
        Vec::new()
    };
    let rows = sqlx::query(
        "SELECT item_id, owner_issuer, owner_subject, citation_key, csl_json,
                created_at_ms, updated_at_ms, revision
         FROM bibliography_items
         WHERE owner_issuer = ? AND owner_subject = ?
         ORDER BY item_id",
    )
    .bind(actor.issuer())
    .bind(actor.subject())
    .fetch_all(&mut **transaction)
    .await
    .map_err(map_database_error)?;
    let items = rows
        .into_iter()
        .map(decode_item)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| BibliographyImportRepositoryError::CorruptData)?;
    if links.iter().any(|link| {
        items
            .iter()
            .find(|item| item.item_id() == link.item_id())
            .is_none_or(|item| link.imported_item_revision() > item.revision())
    }) {
        return Err(BibliographyImportRepositoryError::CorruptData);
    }
    Ok(BibliographyImportState {
        source,
        links,
        items,
    }
    .canonicalized())
}

async fn persist_source(
    transaction: &mut Transaction<'_, Sqlite>,
    actor: &Actor,
    commit: &BibliographyImportCommit,
) -> Result<Revision, BibliographyImportRepositoryError> {
    if let Some(expected_source) = &commit.expected_state.source {
        if expected_source.source_id() != commit.source.source_id() {
            return Err(BibliographyImportRepositoryError::Conflict);
        }
        let expected_revision = expected_source.revision();
        let result = sqlx::query(
            "UPDATE bibliography_import_sources
             SET last_imported_at_ms = ?, revision = revision + 1
             WHERE source_id = ? AND owner_issuer = ? AND owner_subject = ? AND revision = ?",
        )
        .bind(commit.imported_at.get())
        .bind(commit.source.source_id().to_string())
        .bind(actor.issuer())
        .bind(actor.subject())
        .bind(expected_revision.get())
        .execute(&mut **transaction)
        .await
        .map_err(map_write_error)?;
        if result.rows_affected() != 1 {
            return Err(BibliographyImportRepositoryError::Conflict);
        }
        return expected_revision
            .get()
            .checked_add(1)
            .and_then(|value| Revision::new(value).ok())
            .ok_or(BibliographyImportRepositoryError::CorruptData);
    }
    sqlx::query(
        "INSERT INTO bibliography_import_sources (
            source_id, owner_issuer, owner_subject, method, display_name, revision,
            created_at_ms, last_imported_at_ms
         ) VALUES (?, ?, ?, 'csl_json_file', ?, 1, ?, ?)",
    )
    .bind(commit.source.source_id().to_string())
    .bind(actor.issuer())
    .bind(actor.subject())
    .bind(commit.source.display_name())
    .bind(commit.source.created_at().get())
    .bind(commit.imported_at.get())
    .execute(&mut **transaction)
    .await
    .map_err(map_write_error)?;
    Ok(Revision::INITIAL)
}

async fn persist_link(
    transaction: &mut Transaction<'_, Sqlite>,
    actor: &Actor,
    link: &BibliographyImportLink,
) -> Result<(), BibliographyImportRepositoryError> {
    let result = sqlx::query(
        "INSERT INTO bibliography_import_links (
            source_id, external_item_id, item_id, owner_issuer, owner_subject,
            imported_digest, imported_item_revision
         ) VALUES (?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT (source_id, external_item_id) DO UPDATE SET
            imported_digest = excluded.imported_digest,
            imported_item_revision = excluded.imported_item_revision
         WHERE bibliography_import_links.item_id = excluded.item_id
           AND bibliography_import_links.owner_issuer = excluded.owner_issuer
           AND bibliography_import_links.owner_subject = excluded.owner_subject",
    )
    .bind(link.source_id().to_string())
    .bind(link.external_item_id())
    .bind(link.item_id().to_string())
    .bind(actor.issuer())
    .bind(actor.subject())
    .bind(link.imported_digest().as_bytes().as_slice())
    .bind(link.imported_item_revision().get())
    .execute(&mut **transaction)
    .await
    .map_err(map_write_error)?;
    if result.rows_affected() != 1 {
        return Err(BibliographyImportRepositoryError::Conflict);
    }
    Ok(())
}

pub(crate) fn decode_source(
    row: sqlx::sqlite::SqliteRow,
) -> Result<BibliographyImportSource, BibliographyImportRepositoryError> {
    let source_id: String = row.try_get("source_id").map_err(corrupt)?;
    let source_id = EntityId::from_str(&source_id)
        .map(BibliographyImportSourceId::new)
        .map_err(corrupt)?;
    let method: String = row.try_get("method").map_err(corrupt)?;
    let method = match method.as_str() {
        "csl_json_file" => BibliographyImportMethod::CslJsonFile,
        _ => return Err(BibliographyImportRepositoryError::CorruptData),
    };
    BibliographyImportSource::restore(
        source_id,
        Identity::new(
            row.try_get("owner_issuer").map_err(corrupt)?,
            row.try_get("owner_subject").map_err(corrupt)?,
        )
        .map_err(corrupt)?,
        method,
        row.try_get("display_name").map_err(corrupt)?,
        Revision::new(row.try_get("revision").map_err(corrupt)?).map_err(corrupt)?,
        UnixMillis::new(row.try_get("created_at_ms").map_err(corrupt)?),
        UnixMillis::new(row.try_get("last_imported_at_ms").map_err(corrupt)?),
    )
    .map_err(corrupt)
}

pub(crate) fn decode_link(
    row: sqlx::sqlite::SqliteRow,
) -> Result<BibliographyImportLink, BibliographyImportRepositoryError> {
    let source_id: String = row.try_get("source_id").map_err(corrupt)?;
    let item_id: String = row.try_get("item_id").map_err(corrupt)?;
    let digest: Vec<u8> = row.try_get("imported_digest").map_err(corrupt)?;
    let digest: [u8; 32] = digest.try_into().map_err(corrupt)?;
    BibliographyImportLink::new(
        EntityId::from_str(&source_id)
            .map(BibliographyImportSourceId::new)
            .map_err(corrupt)?,
        row.try_get("external_item_id").map_err(corrupt)?,
        EntityId::from_str(&item_id)
            .map(BibliographyItemId::new)
            .map_err(corrupt)?,
        BibliographyContentDigest::new(digest),
        Revision::new(row.try_get("imported_item_revision").map_err(corrupt)?).map_err(corrupt)?,
    )
    .map_err(corrupt)
}

fn map_write_error(error: sqlx::Error) -> BibliographyImportRepositoryError {
    if error.as_database_error().is_some_and(|error| {
        error.is_unique_violation()
            || error.is_foreign_key_violation()
            || error
                .code()
                .and_then(|code| code.parse::<i32>().ok())
                .is_some_and(|code| matches!(code & 0xff, 5 | 6))
    }) {
        BibliographyImportRepositoryError::Conflict
    } else {
        map_database_error(error)
    }
}

fn map_database_error(error: sqlx::Error) -> BibliographyImportRepositoryError {
    let _ = database_error(error);
    BibliographyImportRepositoryError::Unavailable
}

fn corrupt<T>(_: T) -> BibliographyImportRepositoryError {
    BibliographyImportRepositoryError::CorruptData
}
