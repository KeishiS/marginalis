//! CSL-JSON文献ライブラリのSQLite実装。

use std::str::FromStr;

use async_trait::async_trait;
use marginalis_application::{BibliographyRepository, StorageError};
use marginalis_domain::{
    Actor, BibliographyItem, BibliographyItemId, EntityId, Identity, PrincipalId, PrincipalRef,
    Revision, UnixMillis, ValidatedCslJson,
};
use sqlx::Row;

use crate::{SqliteDatabase, storage_error};

#[async_trait]
impl BibliographyRepository for SqliteDatabase {
    async fn search_owned_items(
        &self,
        actor: &Actor,
        query: &str,
    ) -> Result<Vec<BibliographyItem>, StorageError> {
        let pattern = crate::like_contains_pattern(query);
        let rows = sqlx::query(
            "SELECT item_id, owner_principal_id, owner_issuer, owner_subject, citation_key, csl_json,
                    created_at_ms, updated_at_ms, revision
             FROM bibliography_item_details
             WHERE owner_principal_id = ?
               AND (?2 = '' OR lower(citation_key) LIKE ?3 ESCAPE '!'
                            OR lower(csl_json) LIKE ?3 ESCAPE '!')
             ORDER BY updated_at_ms DESC, item_id
             LIMIT 200",
        )
        .bind(actor.principal_id().get())
        .bind(query)
        .bind(&pattern)
        .fetch_all(&self.pool)
        .await
        .map_err(storage_error)?;

        rows.into_iter().map(decode_item).collect()
    }

    async fn items_by_citation_keys(
        &self,
        owner: &PrincipalRef,
        citation_keys: &[String],
    ) -> Result<Vec<BibliographyItem>, StorageError> {
        if citation_keys.is_empty() {
            return Ok(Vec::new());
        }
        // citation keyの数は本文の引用数で決まるため、値の数だけ`?`を並べる。
        let placeholders = vec!["?"; citation_keys.len()].join(", ");
        let statement = format!(
            "SELECT item_id, owner_principal_id, owner_issuer, owner_subject, citation_key, csl_json,
                    created_at_ms, updated_at_ms, revision
             FROM bibliography_item_details
             WHERE owner_principal_id = ? AND citation_key IN ({placeholders})"
        );
        let mut query = sqlx::query(&statement).bind(owner.id().get());
        for citation_key in citation_keys {
            query = query.bind(citation_key.clone());
        }
        let rows = query.fetch_all(&self.pool).await.map_err(storage_error)?;

        rows.into_iter().map(decode_item).collect()
    }

    async fn create_owned_item(&self, item: &BibliographyItem) -> Result<(), StorageError> {
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
        .execute(&self.pool)
        .await
        .map_err(|error| {
            if error
                .as_database_error()
                .is_some_and(|error| error.is_unique_violation())
            {
                StorageError::Conflict
            } else {
                storage_error(error)
            }
        })?;
        Ok(())
    }

    async fn update_owned_item(
        &self,
        actor: &Actor,
        item_id: BibliographyItemId,
        csl_json: &ValidatedCslJson,
        updated_at: UnixMillis,
        expected_revision: Revision,
    ) -> Result<BibliographyItem, StorageError> {
        let result = sqlx::query(
            "UPDATE bibliography_items
             SET citation_key = ?, csl_json = ?, updated_at_ms = ?, revision = revision + 1
             WHERE item_id = ? AND owner_principal_id = ? AND revision = ?",
        )
        .bind(csl_json.citation_key())
        .bind(csl_json.encoded())
        .bind(updated_at.get())
        .bind(item_id.to_string())
        .bind(actor.principal_id().get())
        .bind(expected_revision.get())
        .execute(&self.pool)
        .await
        .map_err(|error| {
            if error
                .as_database_error()
                .is_some_and(|error| error.is_unique_violation())
            {
                StorageError::Conflict
            } else {
                storage_error(error)
            }
        })?;
        if result.rows_affected() == 1 {
            let row = sqlx::query("SELECT * FROM bibliography_item_details WHERE item_id = ?")
                .bind(item_id.to_string())
                .fetch_one(&self.pool)
                .await
                .map_err(storage_error)?;
            return decode_item(row);
        }
        let exists = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM bibliography_items
             WHERE item_id = ? AND owner_principal_id = ?",
        )
        .bind(item_id.to_string())
        .bind(actor.principal_id().get())
        .fetch_one(&self.pool)
        .await
        .map_err(storage_error)?
            != 0;
        Err(if exists {
            StorageError::Conflict
        } else {
            StorageError::NotFound
        })
    }

    async fn delete_owned_item(
        &self,
        actor: &Actor,
        item_id: BibliographyItemId,
        expected_revision: Revision,
    ) -> Result<(), StorageError> {
        let mut transaction = self.pool.begin().await.map_err(storage_error)?;
        let result = sqlx::query(
            "DELETE FROM bibliography_items
             WHERE item_id = ? AND owner_principal_id = ? AND revision = ?",
        )
        .bind(item_id.to_string())
        .bind(actor.principal_id().get())
        .bind(expected_revision.get())
        .execute(&mut *transaction)
        .await
        .map_err(storage_error)?;
        if result.rows_affected() == 1 {
            transaction.commit().await.map_err(storage_error)?;
            return Ok(());
        }
        let exists = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM bibliography_items
             WHERE item_id = ? AND owner_principal_id = ?",
        )
        .bind(item_id.to_string())
        .bind(actor.principal_id().get())
        .fetch_one(&mut *transaction)
        .await
        .map_err(storage_error)?
            != 0;
        Err(if exists {
            StorageError::Conflict
        } else {
            StorageError::NotFound
        })
    }
}

pub(crate) fn decode_item(row: sqlx::sqlite::SqliteRow) -> Result<BibliographyItem, StorageError> {
    let corrupt = |_| StorageError::CorruptData;
    let item_id_text: String = row.try_get("item_id").map_err(corrupt)?;
    let item_id = EntityId::from_str(&item_id_text)
        .map(BibliographyItemId::new)
        .map_err(|_| StorageError::CorruptData)?;
    let owner_identity = Identity::new(
        row.try_get("owner_issuer").map_err(corrupt)?,
        row.try_get("owner_subject").map_err(corrupt)?,
    )
    .map_err(|_| StorageError::CorruptData)?;
    let owner = PrincipalRef::new(
        PrincipalId::new(row.try_get("owner_principal_id").map_err(corrupt)?)
            .map_err(|_| StorageError::CorruptData)?,
        owner_identity,
    );
    let revision = Revision::new(row.try_get("revision").map_err(corrupt)?)
        .map_err(|_| StorageError::CorruptData)?;
    BibliographyItem::restore(
        item_id,
        owner,
        row.try_get("citation_key").map_err(corrupt)?,
        row.try_get("csl_json").map_err(corrupt)?,
        UnixMillis::new(row.try_get("created_at_ms").map_err(corrupt)?),
        UnixMillis::new(row.try_get("updated_at_ms").map_err(corrupt)?),
        revision,
    )
    .map_err(|_| StorageError::CorruptData)
}
