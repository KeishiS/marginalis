//! CSL-JSON書誌ライブラリーのSQLite実装。

use std::str::FromStr;

use async_trait::async_trait;
use marginalis_application::{BibliographyRepository, BibliographyRepositoryError};
use marginalis_domain::{
    Actor, BibliographyItem, BibliographyItemId, EntityId, Identity, Revision, UnixMillis,
};
use sqlx::Row;

use crate::{SqliteDatabase, database_error};

#[async_trait]
impl BibliographyRepository for SqliteDatabase {
    async fn search_owned_items(
        &self,
        actor: &Actor,
        query: &str,
    ) -> Result<Vec<BibliographyItem>, BibliographyRepositoryError> {
        let pattern = format!("%{}%", query.to_lowercase());
        let rows = sqlx::query(
            "SELECT item_id, owner_issuer, owner_subject, citation_key, csl_json,
                    created_at_ms, updated_at_ms, revision
             FROM bibliography_items
             WHERE owner_issuer = ? AND owner_subject = ?
               AND (? = '' OR lower(citation_key) LIKE ? OR lower(csl_json) LIKE ?)
             ORDER BY updated_at_ms DESC, item_id
             LIMIT 200",
        )
        .bind(actor.issuer())
        .bind(actor.subject())
        .bind(query)
        .bind(&pattern)
        .bind(&pattern)
        .fetch_all(&self.pool)
        .await
        .map_err(|error| map_error(database_error(error)))?;

        rows.into_iter().map(decode_item).collect()
    }

    async fn create_owned_item(
        &self,
        item: &BibliographyItem,
    ) -> Result<(), BibliographyRepositoryError> {
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
        .execute(&self.pool)
        .await
        .map_err(|error| {
            if error
                .as_database_error()
                .is_some_and(|error| error.is_unique_violation())
            {
                BibliographyRepositoryError::Conflict
            } else {
                map_error(database_error(error))
            }
        })?;
        Ok(())
    }

    async fn update_owned_item(
        &self,
        actor: &Actor,
        item_id: BibliographyItemId,
        citation_key: &str,
        csl_json: &str,
        updated_at: UnixMillis,
        expected_revision: Revision,
    ) -> Result<BibliographyItem, BibliographyRepositoryError> {
        let row = sqlx::query(
            "UPDATE bibliography_items
             SET citation_key = ?, csl_json = ?, updated_at_ms = ?, revision = revision + 1
             WHERE item_id = ? AND owner_issuer = ? AND owner_subject = ? AND revision = ?
             RETURNING item_id, owner_issuer, owner_subject, citation_key, csl_json,
                       created_at_ms, updated_at_ms, revision",
        )
        .bind(citation_key)
        .bind(csl_json)
        .bind(updated_at.get())
        .bind(item_id.to_string())
        .bind(actor.issuer())
        .bind(actor.subject())
        .bind(expected_revision.get())
        .fetch_optional(&self.pool)
        .await
        .map_err(|error| {
            if error
                .as_database_error()
                .is_some_and(|error| error.is_unique_violation())
            {
                BibliographyRepositoryError::Conflict
            } else {
                map_error(database_error(error))
            }
        })?;
        if let Some(row) = row {
            return decode_item(row);
        }
        let exists = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM bibliography_items
             WHERE item_id = ? AND owner_issuer = ? AND owner_subject = ?",
        )
        .bind(item_id.to_string())
        .bind(actor.issuer())
        .bind(actor.subject())
        .fetch_one(&self.pool)
        .await
        .map_err(|error| map_error(database_error(error)))?
            != 0;
        Err(if exists {
            BibliographyRepositoryError::Conflict
        } else {
            BibliographyRepositoryError::NotFound
        })
    }

    async fn delete_owned_item(
        &self,
        actor: &Actor,
        item_id: BibliographyItemId,
        expected_revision: Revision,
    ) -> Result<(), BibliographyRepositoryError> {
        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(|error| map_error(database_error(error)))?;
        let result = sqlx::query(
            "DELETE FROM bibliography_items
             WHERE item_id = ? AND owner_issuer = ? AND owner_subject = ? AND revision = ?",
        )
        .bind(item_id.to_string())
        .bind(actor.issuer())
        .bind(actor.subject())
        .bind(expected_revision.get())
        .execute(&mut *transaction)
        .await
        .map_err(|error| map_error(database_error(error)))?;
        if result.rows_affected() == 1 {
            transaction
                .commit()
                .await
                .map_err(|error| map_error(database_error(error)))?;
            return Ok(());
        }
        let exists = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM bibliography_items
             WHERE item_id = ? AND owner_issuer = ? AND owner_subject = ?",
        )
        .bind(item_id.to_string())
        .bind(actor.issuer())
        .bind(actor.subject())
        .fetch_one(&mut *transaction)
        .await
        .map_err(|error| map_error(database_error(error)))?
            != 0;
        Err(if exists {
            BibliographyRepositoryError::Conflict
        } else {
            BibliographyRepositoryError::NotFound
        })
    }
}

pub(crate) fn decode_item(
    row: sqlx::sqlite::SqliteRow,
) -> Result<BibliographyItem, BibliographyRepositoryError> {
    let item_id = EntityId::from_str(row.get::<String, _>("item_id").as_str())
        .map(BibliographyItemId::new)
        .map_err(|_| BibliographyRepositoryError::CorruptData)?;
    let owner = Identity::new(row.get("owner_issuer"), row.get("owner_subject"))
        .map_err(|_| BibliographyRepositoryError::CorruptData)?;
    let revision =
        Revision::new(row.get("revision")).map_err(|_| BibliographyRepositoryError::CorruptData)?;
    BibliographyItem::restore(
        item_id,
        owner,
        row.get("citation_key"),
        row.get("csl_json"),
        UnixMillis::new(row.get("created_at_ms")),
        UnixMillis::new(row.get("updated_at_ms")),
        revision,
    )
    .map_err(|_| BibliographyRepositoryError::CorruptData)
}

fn map_error(error: crate::SqliteStoreError) -> BibliographyRepositoryError {
    match error {
        crate::SqliteStoreError::NotFound => BibliographyRepositoryError::NotFound,
        crate::SqliteStoreError::Conflict => BibliographyRepositoryError::Conflict,
        crate::SqliteStoreError::CorruptData | crate::SqliteStoreError::ArchiveTargetNotEmpty => {
            BibliographyRepositoryError::CorruptData
        }
        crate::SqliteStoreError::Database(_) => BibliographyRepositoryError::Unavailable,
    }
}
