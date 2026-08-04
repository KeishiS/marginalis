use async_trait::async_trait;
use marginalis_application::{
    MathMacro, MathMacroRepository, MathMacroRepositoryError, MathMacroSettings,
    validate_math_macros,
};
use marginalis_domain::Identity;
use serde::{Deserialize, Serialize};
use sqlx::Row;

use crate::{SqliteDatabase, database_error};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StoredMathMacro {
    pub(crate) name: String,
    pub(crate) replacement: String,
    pub(crate) argument_count: u8,
}

#[async_trait]
impl MathMacroRepository for SqliteDatabase {
    async fn read_math_macros(
        &self,
        owner: &Identity,
    ) -> Result<MathMacroSettings, MathMacroRepositoryError> {
        let row = sqlx::query(
            "SELECT macros_json, revision FROM math_macro_settings
             WHERE owner_issuer = ? AND owner_subject = ?",
        )
        .bind(owner.issuer())
        .bind(owner.subject())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_database_error)?;
        row.map_or_else(|| Ok(MathMacroSettings::default()), decode_settings)
    }

    async fn replace_math_macros(
        &self,
        owner: &Identity,
        macros: &[MathMacro],
        expected_revision: i64,
    ) -> Result<MathMacroSettings, MathMacroRepositoryError> {
        let stored = macros
            .iter()
            .map(|item| StoredMathMacro {
                name: item.name.clone(),
                replacement: item.replacement.clone(),
                argument_count: item.argument_count,
            })
            .collect::<Vec<_>>();
        let encoded =
            serde_json::to_string(&stored).map_err(|_| MathMacroRepositoryError::Unavailable)?;
        let mut transaction = self.pool.begin().await.map_err(map_database_error)?;
        let revision = if expected_revision == 0 {
            let result = sqlx::query(
                "INSERT INTO math_macro_settings (
                    owner_issuer, owner_subject, macros_json, revision
                 ) VALUES (?, ?, ?, 1)
                 ON CONFLICT (owner_issuer, owner_subject) DO NOTHING",
            )
            .bind(owner.issuer())
            .bind(owner.subject())
            .bind(&encoded)
            .execute(&mut *transaction)
            .await
            .map_err(map_database_error)?;
            if result.rows_affected() != 1 {
                return Err(MathMacroRepositoryError::Conflict);
            }
            1
        } else {
            let row = sqlx::query(
                "UPDATE math_macro_settings
                 SET macros_json = ?, revision = revision + 1
                 WHERE owner_issuer = ? AND owner_subject = ? AND revision = ?
                 RETURNING revision",
            )
            .bind(&encoded)
            .bind(owner.issuer())
            .bind(owner.subject())
            .bind(expected_revision)
            .fetch_optional(&mut *transaction)
            .await
            .map_err(map_database_error)?;
            let Some(row) = row else {
                return Err(MathMacroRepositoryError::Conflict);
            };
            row.try_get("revision")
                .map_err(|_| MathMacroRepositoryError::CorruptData)?
        };
        transaction.commit().await.map_err(map_database_error)?;
        Ok(MathMacroSettings {
            macros: macros.to_vec(),
            revision,
        })
    }
}

pub(crate) fn decode_settings(
    row: sqlx::sqlite::SqliteRow,
) -> Result<MathMacroSettings, MathMacroRepositoryError> {
    let encoded: String = row
        .try_get("macros_json")
        .map_err(|_| MathMacroRepositoryError::CorruptData)?;
    let revision: i64 = row
        .try_get("revision")
        .map_err(|_| MathMacroRepositoryError::CorruptData)?;
    if revision <= 0 {
        return Err(MathMacroRepositoryError::CorruptData);
    }
    let stored: Vec<StoredMathMacro> =
        serde_json::from_str(&encoded).map_err(|_| MathMacroRepositoryError::CorruptData)?;
    let macros = stored
        .into_iter()
        .map(|item| MathMacro {
            name: item.name,
            replacement: item.replacement,
            argument_count: item.argument_count,
        })
        .collect::<Vec<_>>();
    validate_math_macros(&macros).map_err(|_| MathMacroRepositoryError::CorruptData)?;
    Ok(MathMacroSettings { macros, revision })
}

fn map_database_error(error: sqlx::Error) -> MathMacroRepositoryError {
    match database_error(error) {
        crate::SqliteStoreError::CorruptData => MathMacroRepositoryError::CorruptData,
        crate::SqliteStoreError::NotFound
        | crate::SqliteStoreError::Conflict
        | crate::SqliteStoreError::ArchiveTargetNotEmpty
        | crate::SqliteStoreError::Database(_) => MathMacroRepositoryError::Unavailable,
    }
}
