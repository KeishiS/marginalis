//! 外部identityと内部principalの対応。

use async_trait::async_trait;
use marginalis_application::{PrincipalDirectory, StorageError};
use marginalis_domain::{Actor, Identity, Principal, PrincipalId, PrincipalRef};
use sqlx::{Row, Sqlite, Transaction};

use crate::{SqliteDatabase, storage_error};

#[async_trait]
impl PrincipalDirectory for SqliteDatabase {
    async fn resolve_or_create_verified(&self, identity: Identity) -> Result<Actor, StorageError> {
        resolve_or_create(self, identity)
            .await
            .map(|(actor, _)| actor)
    }

    async fn resolve(&self, identity: &Identity) -> Result<Option<Actor>, StorageError> {
        let mut transaction = self.pool.begin().await.map_err(storage_error)?;
        let resolved = resolve_in(&mut transaction, identity).await?;
        transaction.commit().await.map_err(storage_error)?;
        Ok(resolved.map(|(actor, _)| actor))
    }

    async fn resolve_or_create_acl_target(
        &self,
        identity: Identity,
    ) -> Result<PrincipalRef, StorageError> {
        resolve_or_create(self, identity)
            .await
            .map(|(actor, _)| actor.principal().clone())
    }
}

pub(crate) async fn resolve_or_create(
    database: &SqliteDatabase,
    identity: Identity,
) -> Result<(Actor, i64), StorageError> {
    // 初回ログインが同時に到着しても、双方が未登録と読んだ後で競合しないよう、
    // identityの確認より前に書込み予約を取る。
    let mut transaction = database
        .pool
        .begin_with("BEGIN IMMEDIATE")
        .await
        .map_err(storage_error)?;
    let resolved = resolve_or_create_in(&mut transaction, identity).await?;
    transaction.commit().await.map_err(storage_error)?;
    Ok(resolved)
}

pub(crate) async fn resolve_or_create_in(
    transaction: &mut Transaction<'_, Sqlite>,
    identity: Identity,
) -> Result<(Actor, i64), StorageError> {
    if let Some(resolved) = resolve_in(transaction, &identity).await? {
        return Ok(resolved);
    }
    let principal_id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO principals DEFAULT VALUES RETURNING principal_id",
    )
    .fetch_one(&mut **transaction)
    .await
    .map_err(storage_error)?;
    let identity_id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO principal_identities (principal_id, issuer, subject, is_primary)
         VALUES (?, ?, ?, 1)
         RETURNING identity_id",
    )
    .bind(principal_id)
    .bind(identity.issuer())
    .bind(identity.subject())
    .fetch_one(&mut **transaction)
    .await
    .map_err(storage_error)?;
    let principal_id = PrincipalId::new(principal_id).map_err(|_| StorageError::CorruptData)?;
    let actor = Actor::for_single_identity(principal_id, identity);
    Ok((actor, identity_id))
}

pub(crate) async fn resolve_in(
    transaction: &mut Transaction<'_, Sqlite>,
    identity: &Identity,
) -> Result<Option<(Actor, i64)>, StorageError> {
    let row = sqlx::query(
        "SELECT matched.identity_id, matched.principal_id,
                primary_identity.issuer AS primary_issuer,
                primary_identity.subject AS primary_subject
         FROM principal_identities matched
         LEFT JOIN principal_identities primary_identity
           ON primary_identity.principal_id = matched.principal_id
          AND primary_identity.is_primary = 1
         WHERE matched.issuer = ? AND matched.subject = ?",
    )
    .bind(identity.issuer())
    .bind(identity.subject())
    .fetch_optional(&mut **transaction)
    .await
    .map_err(storage_error)?;
    let Some(row) = row else {
        return Ok(None);
    };
    let principal_id = PrincipalId::new(
        row.try_get("principal_id")
            .map_err(|_| StorageError::CorruptData)?,
    )
    .map_err(|_| StorageError::CorruptData)?;
    let primary_issuer = row
        .try_get::<Option<String>, _>("primary_issuer")
        .map_err(|_| StorageError::CorruptData)?
        .ok_or(StorageError::CorruptData)?;
    let primary_subject = row
        .try_get::<Option<String>, _>("primary_subject")
        .map_err(|_| StorageError::CorruptData)?
        .ok_or(StorageError::CorruptData)?;
    let primary_identity =
        Identity::new(primary_issuer, primary_subject).map_err(|_| StorageError::CorruptData)?;
    let identities = if primary_identity == *identity {
        vec![identity.clone()]
    } else {
        vec![primary_identity.clone(), identity.clone()]
    };
    let principal = Principal::restore(principal_id, primary_identity, identities)
        .map_err(|_| StorageError::CorruptData)?;
    let actor =
        Actor::authenticate(principal, identity.clone()).map_err(|_| StorageError::CorruptData)?;
    let identity_id = row
        .try_get("identity_id")
        .map_err(|_| StorageError::CorruptData)?;
    Ok(Some((actor, identity_id)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn concurrent_first_login_creates_one_principal() {
        let database = crate::tests::database().await;
        let identity = Identity::new(
            "https://id.example.test".into(),
            "simultaneous-first-login".into(),
        )
        .expect("identity");
        let left_database = database.clone();
        let left_identity = identity.clone();
        let right_database = database.clone();
        let right_identity = identity.clone();
        let (left, right) = tokio::join!(
            left_database.resolve_or_create_verified(left_identity),
            right_database.resolve_or_create_verified(right_identity),
        );
        let left = left.expect("first login");
        let right = right.expect("second login");
        assert_eq!(left.principal_id(), right.principal_id());
        let count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM principal_identities WHERE issuer = ? AND subject = ?",
        )
        .bind(identity.issuer())
        .bind(identity.subject())
        .fetch_one(&database.pool)
        .await
        .expect("identity count");
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn resolving_an_unknown_saved_identity_does_not_create_it() {
        let database = crate::tests::database().await;
        let identity = Identity::new(
            "https://id.example.test".into(),
            "unknown-session-identity".into(),
        )
        .expect("identity");
        assert_eq!(database.resolve(&identity).await.expect("resolve"), None);
        let count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM principal_identities WHERE issuer = ? AND subject = ?",
        )
        .bind(identity.issuer())
        .bind(identity.subject())
        .fetch_one(&database.pool)
        .await
        .expect("identity count");
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn resolving_an_identity_without_a_primary_identity_fails_closed() {
        let database = crate::tests::database().await;
        let identity =
            Identity::new("https://id.example.test".into(), "alice".into()).expect("identity");
        sqlx::query(
            "UPDATE principal_identities SET is_primary = 0
             WHERE issuer = ? AND subject = ?",
        )
        .bind(identity.issuer())
        .bind(identity.subject())
        .execute(&database.pool)
        .await
        .expect("corrupt principal fixture");

        assert_eq!(
            database.resolve(&identity).await,
            Err(StorageError::CorruptData)
        );
    }
}
