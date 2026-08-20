//! service停止中に行う、外部identityの明示的な引き継ぎ。

use std::path::{Path, PathBuf};

use marginalis_domain::Identity;
use sqlx::{Connection, Row as _};

use crate::{
    migration::{
        DatabaseMigrationError, open_exclusive_database, publish_verified_backup,
        read_schema_history, verify_database,
    },
    schema::{MIGRATIONS, SCHEMA_VERSION, validate_schema_history},
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum IdentityMaintenanceRequest {
    Link {
        existing: Identity,
        new_identity: Identity,
        make_primary: bool,
    },
    SetPrimary {
        identity: Identity,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IdentityMaintenanceReport {
    pub backup_path: PathBuf,
    pub identity_linked: bool,
    pub primary_changed: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum IdentityMaintenanceError {
    #[error(transparent)]
    Backup(#[from] DatabaseMigrationError),
    #[error("existing identity was not found")]
    ExistingIdentityNotFound,
    #[error("new identity must differ from the existing identity")]
    DuplicateIdentity,
    #[error("new identity is already bound to a principal")]
    IdentityAlreadyBound,
    #[error("identity is already the primary identity")]
    AlreadyPrimary,
    #[error("stored principal identities are inconsistent")]
    CorruptData,
    #[error("identity maintenance failed: {0}")]
    Database(#[from] sqlx::Error),
}

pub(crate) async fn maintain_identity(
    database_url: &str,
    backup_path: &Path,
    request: IdentityMaintenanceRequest,
) -> Result<IdentityMaintenanceReport, IdentityMaintenanceError> {
    if let IdentityMaintenanceRequest::Link {
        existing,
        new_identity,
        ..
    } = &request
        && existing == new_identity
    {
        return Err(IdentityMaintenanceError::DuplicateIdentity);
    }

    let (mut connection, backup_path) = open_exclusive_database(database_url, backup_path).await?;
    sqlx::query("PRAGMA foreign_keys = ON")
        .execute(&mut connection)
        .await?;
    let history = read_schema_history(&mut connection).await?;
    validate_schema_history(&history, MIGRATIONS, false)?;
    verify_database(&mut connection).await?;
    verify_principal_bindings(&mut connection).await?;

    let (principal_id, identity_linked, primary_changed) = match &request {
        IdentityMaintenanceRequest::Link {
            existing,
            new_identity,
            make_primary,
        } => {
            let (principal_id, _) = find_identity(&mut connection, existing)
                .await?
                .ok_or(IdentityMaintenanceError::ExistingIdentityNotFound)?;
            if find_identity(&mut connection, new_identity)
                .await?
                .is_some()
            {
                return Err(IdentityMaintenanceError::IdentityAlreadyBound);
            }
            (principal_id, true, *make_primary)
        }
        IdentityMaintenanceRequest::SetPrimary { identity } => {
            let (principal_id, is_primary) = find_identity(&mut connection, identity)
                .await?
                .ok_or(IdentityMaintenanceError::ExistingIdentityNotFound)?;
            if is_primary {
                return Err(IdentityMaintenanceError::AlreadyPrimary);
            }
            (principal_id, false, true)
        }
    };

    // 入力、schema、保存済みbindingをすべて検証した後、変更より先にSQLite全体を退避する。
    publish_verified_backup(
        &mut connection,
        &backup_path,
        &history,
        MIGRATIONS,
        SCHEMA_VERSION,
    )
    .await?;

    let mut transaction = connection.begin().await?;
    match &request {
        IdentityMaintenanceRequest::Link {
            new_identity,
            make_primary,
            ..
        } => {
            sqlx::query(
                "INSERT INTO principal_identities
                     (principal_id, issuer, subject, is_primary)
                 VALUES (?, ?, ?, 0)",
            )
            .bind(principal_id)
            .bind(new_identity.issuer())
            .bind(new_identity.subject())
            .execute(&mut *transaction)
            .await?;
            if *make_primary {
                set_primary(&mut transaction, principal_id, new_identity).await?;
            }
        }
        IdentityMaintenanceRequest::SetPrimary { identity } => {
            set_primary(&mut transaction, principal_id, identity).await?;
        }
    }
    verify_principal_bindings(&mut *transaction).await?;
    verify_database(&mut transaction).await?;
    transaction.commit().await?;
    verify_database(&mut connection).await?;
    verify_principal_bindings(&mut connection).await?;

    Ok(IdentityMaintenanceReport {
        backup_path,
        identity_linked,
        primary_changed,
    })
}

async fn find_identity(
    connection: &mut sqlx::SqliteConnection,
    identity: &Identity,
) -> Result<Option<(i64, bool)>, sqlx::Error> {
    sqlx::query(
        "SELECT principal_id, is_primary
         FROM principal_identities
         WHERE issuer = ? AND subject = ?",
    )
    .bind(identity.issuer())
    .bind(identity.subject())
    .fetch_optional(connection)
    .await?
    .map(|row| {
        let is_primary = row.try_get::<i64, _>("is_primary")?;
        if !matches!(is_primary, 0 | 1) {
            return Err(sqlx::Error::Protocol(
                "principal identity has an invalid primary marker".into(),
            ));
        }
        Ok((row.try_get("principal_id")?, is_primary == 1))
    })
    .transpose()
}

async fn set_primary(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    principal_id: i64,
    identity: &Identity,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE principal_identities SET is_primary = 0 WHERE principal_id = ?")
        .bind(principal_id)
        .execute(&mut **transaction)
        .await?;
    let updated = sqlx::query(
        "UPDATE principal_identities SET is_primary = 1
         WHERE principal_id = ? AND issuer = ? AND subject = ?",
    )
    .bind(principal_id)
    .bind(identity.issuer())
    .bind(identity.subject())
    .execute(&mut **transaction)
    .await?;
    if updated.rows_affected() != 1 {
        return Err(sqlx::Error::Protocol(
            "primary identity target disappeared during maintenance".into(),
        ));
    }
    Ok(())
}

async fn verify_principal_bindings<'e, E>(executor: E) -> Result<(), IdentityMaintenanceError>
where
    E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
{
    let invalid = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*)
         FROM principals principal
         LEFT JOIN principal_identities identity
           ON identity.principal_id = principal.principal_id
         GROUP BY principal.principal_id
         HAVING COUNT(identity.identity_id) = 0
            OR SUM(CASE WHEN identity.is_primary = 1 THEN 1 ELSE 0 END) != 1
         LIMIT 1",
    )
    .fetch_optional(executor)
    .await?;
    if invalid.is_some() {
        return Err(IdentityMaintenanceError::CorruptData);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{fs, os::unix::fs::PermissionsExt as _};

    use sqlx::sqlite::SqliteConnectOptions;

    use super::*;
    use crate::SqliteDatabase;

    fn paths(name: &str) -> (PathBuf, PathBuf, PathBuf) {
        let directory = std::env::temp_dir().join(format!(
            "marginalis-identity-maintenance-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir(&directory).expect("test directory");
        let database = directory.join("database.sqlite3");
        let backup = directory.join("backup.sqlite3");
        (directory, database, backup)
    }

    fn identity(issuer: &str, subject: &str) -> Identity {
        Identity::new(issuer.into(), subject.into()).expect("test identity")
    }

    async fn seed_database(path: &Path) -> String {
        let database_url = format!("sqlite://{}?mode=rwc", path.display());
        let database = SqliteDatabase::connect(&database_url)
            .await
            .expect("test database");
        sqlx::raw_sql(
            "INSERT INTO principals (principal_id) VALUES (1), (2);
             INSERT INTO principal_identities
                 (principal_id, issuer, subject, is_primary)
             VALUES
                 (1, 'https://old-id.example.test', 'alice', 1),
                 (2, 'https://id.example.test', 'bob', 1);",
        )
        .execute(&database.pool)
        .await
        .expect("principal fixture");
        database.pool.close().await;
        database_url
    }

    #[tokio::test]
    async fn linking_and_primary_switch_preserve_the_principal_after_a_verified_backup() {
        let (directory, database_path, backup_path) = paths("link");
        let database_url = seed_database(&database_path).await;
        let old = identity("https://old-id.example.test", "alice");
        let new_identity = identity("https://new-id.example.test", "alice-v2");

        let report = maintain_identity(
            &database_url,
            &backup_path,
            IdentityMaintenanceRequest::Link {
                existing: old.clone(),
                new_identity: new_identity.clone(),
                make_primary: true,
            },
        )
        .await
        .expect("link identity");
        assert_eq!(report.backup_path, backup_path);
        assert!(report.identity_linked);
        assert!(report.primary_changed);
        assert_eq!(
            fs::metadata(&backup_path)
                .expect("backup metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );

        let options = SqliteConnectOptions::new()
            .filename(&database_path)
            .create_if_missing(false)
            .foreign_keys(true);
        let mut connection = sqlx::SqliteConnection::connect_with(&options)
            .await
            .expect("linked database");
        let bindings = sqlx::query_as::<_, (i64, String, String, i64)>(
            "SELECT principal_id, issuer, subject, is_primary
             FROM principal_identities WHERE principal_id = 1 ORDER BY issuer",
        )
        .fetch_all(&mut connection)
        .await
        .expect("bindings");
        assert_eq!(
            bindings,
            vec![
                (
                    1,
                    new_identity.issuer().into(),
                    new_identity.subject().into(),
                    1
                ),
                (1, old.issuer().into(), old.subject().into(), 0),
            ]
        );

        let backup_options = SqliteConnectOptions::new()
            .filename(&backup_path)
            .create_if_missing(false)
            .read_only(true)
            .foreign_keys(true);
        let mut backup = sqlx::SqliteConnection::connect_with(&backup_options)
            .await
            .expect("backup database");
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM principal_identities WHERE issuer = ? AND subject = ?",
            )
            .bind(new_identity.issuer())
            .bind(new_identity.subject())
            .fetch_one(&mut backup)
            .await
            .expect("backup binding count"),
            0
        );
        connection.close().await.expect("close database");
        backup.close().await.expect("close backup");
        fs::remove_dir_all(directory).expect("remove test directory");
    }

    #[tokio::test]
    async fn an_existing_binding_conflict_is_rejected_without_backup_or_change() {
        let (directory, database_path, backup_path) = paths("conflict");
        let database_url = seed_database(&database_path).await;
        let error = maintain_identity(
            &database_url,
            &backup_path,
            IdentityMaintenanceRequest::Link {
                existing: identity("https://old-id.example.test", "alice"),
                new_identity: identity("https://id.example.test", "bob"),
                make_primary: false,
            },
        )
        .await
        .expect_err("bound identity conflict");
        assert!(matches!(
            error,
            IdentityMaintenanceError::IdentityAlreadyBound
        ));
        assert!(!backup_path.exists());

        let database = SqliteDatabase::connect(&database_url)
            .await
            .expect("unchanged database");
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM principal_identities")
                .fetch_one(&database.pool)
                .await
                .expect("identity count"),
            2
        );
        database.pool.close().await;
        fs::remove_dir_all(directory).expect("remove test directory");
    }

    #[tokio::test]
    async fn an_existing_alias_can_be_selected_as_primary_in_a_separate_operation() {
        let (directory, database_path, first_backup) = paths("set-primary");
        let database_url = seed_database(&database_path).await;
        let alias = identity("https://new-id.example.test", "alice-v2");
        maintain_identity(
            &database_url,
            &first_backup,
            IdentityMaintenanceRequest::Link {
                existing: identity("https://old-id.example.test", "alice"),
                new_identity: alias.clone(),
                make_primary: false,
            },
        )
        .await
        .expect("link alias");
        let second_backup = directory.join("primary-backup.sqlite3");
        let report = maintain_identity(
            &database_url,
            &second_backup,
            IdentityMaintenanceRequest::SetPrimary {
                identity: alias.clone(),
            },
        )
        .await
        .expect("set primary identity");
        assert!(!report.identity_linked);
        assert!(report.primary_changed);
        assert!(second_backup.is_file());

        let database = SqliteDatabase::connect(&database_url)
            .await
            .expect("updated database");
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT is_primary FROM principal_identities WHERE issuer = ? AND subject = ?",
            )
            .bind(alias.issuer())
            .bind(alias.subject())
            .fetch_one(&database.pool)
            .await
            .expect("primary marker"),
            1
        );
        database.pool.close().await;
        fs::remove_dir_all(directory).expect("remove test directory");
    }
}
