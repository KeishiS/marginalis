//! 検証済みSQLite退避を伴う、明示的な前進migration。

use crate::schema::{
    INITIAL_SCHEMA, MIGRATIONS, Migration, SCHEMA_VERSION, validate_schema_history_for,
};
use sqlx::{
    Connection, SqliteConnection,
    sqlite::{SqliteConnectOptions, SqliteLockingMode},
};
use std::{
    fs::{DirBuilder, File},
    io,
    os::unix::fs::{DirBuilderExt, PermissionsExt},
    path::{Path, PathBuf},
    str::FromStr,
    time::Duration,
};

const PRIVATE_FILE_MODE: u32 = 0o600;
const PRIVATE_DIRECTORY_MODE: u32 = 0o700;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DatabaseMigrationReport {
    pub from_version: i64,
    pub to_version: i64,
    pub applied_migrations: usize,
    pub backup_path: Option<PathBuf>,
}

#[derive(Debug, thiserror::Error)]
pub enum DatabaseMigrationError {
    #[error("database migration requires an existing file-backed database at an absolute path")]
    InvalidDatabasePath,
    #[error("database migration backup must be a distinct absolute file in an existing directory")]
    InvalidBackupPath,
    #[error("database migration backup already exists")]
    BackupAlreadyExists,
    #[error("database migration backup could not be created: {0}")]
    BackupIo(#[source] io::Error),
    #[error("database migration failed: {0}")]
    Database(#[from] sqlx::Error),
}

pub(crate) async fn migrate_database(
    database_url: &str,
    backup_path: &Path,
) -> Result<DatabaseMigrationReport, DatabaseMigrationError> {
    migrate_database_with(database_url, backup_path, MIGRATIONS, SCHEMA_VERSION).await
}

async fn migrate_database_with(
    database_url: &str,
    backup_path: &Path,
    migrations: &[Migration],
    current_version: i64,
) -> Result<DatabaseMigrationReport, DatabaseMigrationError> {
    let (mut connection, backup_path) = open_exclusive_database(database_url, backup_path).await?;

    let source_history = read_schema_history(&mut connection).await?;
    let applied = validate_schema_history_for(&source_history, migrations, current_version, true)?;
    verify_database(&mut connection).await?;
    let from_version = *source_history
        .last()
        .ok_or_else(|| sqlx::Error::Protocol("database schema history is empty".into()))?;
    let pending = &migrations[applied..];
    if pending.is_empty() {
        return Ok(DatabaseMigrationReport {
            from_version,
            to_version: current_version,
            applied_migrations: 0,
            backup_path: None,
        });
    }

    publish_verified_backup(
        &mut connection,
        &backup_path,
        &source_history,
        migrations,
        current_version,
    )
    .await?;

    sqlx::query("PRAGMA foreign_keys = OFF")
        .execute(&mut connection)
        .await?;
    let mut transaction = connection.begin().await?;
    for migration in pending {
        sqlx::raw_sql(migration.sql)
            .execute(&mut *transaction)
            .await?;
        if migration.rebuild_current_schema {
            sqlx::raw_sql(INITIAL_SCHEMA)
                .execute(&mut *transaction)
                .await?;
            sqlx::raw_sql(migration.copy_sql)
                .execute(&mut *transaction)
                .await?;
        }
        sqlx::query("INSERT INTO schema_migrations (version) VALUES (?)")
            .bind(migration.to)
            .execute(&mut *transaction)
            .await?;
    }
    let migrated_history = read_schema_history(&mut transaction).await?;
    validate_schema_history_for(&migrated_history, migrations, current_version, false)?;
    verify_database(&mut transaction).await?;
    transaction.commit().await?;

    sqlx::query("PRAGMA foreign_keys = ON")
        .execute(&mut connection)
        .await?;
    let foreign_keys_enabled = sqlx::query_scalar::<_, i64>("PRAGMA foreign_keys")
        .fetch_one(&mut connection)
        .await?;
    if foreign_keys_enabled != 1 {
        return Err(sqlx::Error::Protocol(
            "database foreign key enforcement was not restored".into(),
        )
        .into());
    }
    verify_database(&mut connection).await?;

    Ok(DatabaseMigrationReport {
        from_version,
        to_version: current_version,
        applied_migrations: pending.len(),
        backup_path: Some(backup_path),
    })
}

pub(crate) async fn open_exclusive_database(
    database_url: &str,
    backup_path: &Path,
) -> Result<(SqliteConnection, PathBuf), DatabaseMigrationError> {
    let options =
        SqliteConnectOptions::from_str(database_url).map_err(DatabaseMigrationError::Database)?;
    let database_path = options.get_filename();
    if !database_path.is_absolute() || !database_path.is_file() {
        return Err(DatabaseMigrationError::InvalidDatabasePath);
    }
    let database_path = database_path
        .canonicalize()
        .map_err(DatabaseMigrationError::BackupIo)?;
    let backup_path = normalized_new_backup_path(backup_path)?;
    if backup_path == database_path {
        return Err(DatabaseMigrationError::InvalidBackupPath);
    }

    let options = options
        .create_if_missing(false)
        .read_only(false)
        .foreign_keys(false)
        .locking_mode(SqliteLockingMode::Exclusive)
        .busy_timeout(Duration::from_secs(5));
    let mut connection = SqliteConnection::connect_with(&options).await?;

    // EXCLUSIVE locking modeで最初のlockを取得し、connectionを閉じるまで保持する。
    // VACUUM INTOはtransaction内で実行できないため、空transactionでlockだけを確定する。
    sqlx::query("BEGIN EXCLUSIVE")
        .execute(&mut connection)
        .await?;
    sqlx::query("ROLLBACK").execute(&mut connection).await?;
    Ok((connection, backup_path))
}

pub(crate) async fn publish_verified_backup(
    connection: &mut SqliteConnection,
    backup_path: &Path,
    source_history: &[i64],
    migrations: &[Migration],
    current_version: i64,
) -> Result<(), DatabaseMigrationError> {
    let pending_snapshot = PendingSnapshot::create(backup_path)?;
    let snapshot_path = pending_snapshot.snapshot_path().to_owned();
    let snapshot_value = snapshot_path
        .to_str()
        .ok_or(DatabaseMigrationError::InvalidBackupPath)?;
    sqlx::query("VACUUM INTO ?")
        .bind(snapshot_value)
        .execute(&mut *connection)
        .await?;
    std::fs::set_permissions(
        &snapshot_path,
        std::fs::Permissions::from_mode(PRIVATE_FILE_MODE),
    )
    .map_err(DatabaseMigrationError::BackupIo)?;
    verify_snapshot(&snapshot_path, source_history, migrations, current_version).await?;
    pending_snapshot.publish()?;
    Ok(())
}

fn normalized_new_backup_path(path: &Path) -> Result<PathBuf, DatabaseMigrationError> {
    if !path.is_absolute() || path.file_name().is_none() {
        return Err(DatabaseMigrationError::InvalidBackupPath);
    }
    if path.exists() {
        return Err(DatabaseMigrationError::BackupAlreadyExists);
    }
    let parent = path
        .parent()
        .ok_or(DatabaseMigrationError::InvalidBackupPath)?;
    if !parent.is_dir() {
        return Err(DatabaseMigrationError::InvalidBackupPath);
    }
    let parent = parent
        .canonicalize()
        .map_err(DatabaseMigrationError::BackupIo)?;
    Ok(parent.join(
        path.file_name()
            .ok_or(DatabaseMigrationError::InvalidBackupPath)?,
    ))
}

pub(crate) async fn read_schema_history(
    connection: &mut SqliteConnection,
) -> Result<Vec<i64>, sqlx::Error> {
    sqlx::query_scalar::<_, i64>("SELECT version FROM schema_migrations ORDER BY version ASC")
        .fetch_all(connection)
        .await
}

pub(crate) async fn verify_database(connection: &mut SqliteConnection) -> Result<(), sqlx::Error> {
    let integrity = sqlx::query_scalar::<_, String>("PRAGMA quick_check")
        .fetch_all(&mut *connection)
        .await?;
    if integrity.as_slice() != ["ok"] {
        return Err(sqlx::Error::Protocol(
            "database quick_check reported corruption".into(),
        ));
    }
    let foreign_key_violations = sqlx::query("PRAGMA foreign_key_check")
        .fetch_all(&mut *connection)
        .await?;
    if !foreign_key_violations.is_empty() {
        return Err(sqlx::Error::Protocol(
            "database foreign_key_check reported violations".into(),
        ));
    }
    Ok(())
}

async fn verify_snapshot(
    path: &Path,
    expected_history: &[i64],
    migrations: &[Migration],
    current_version: i64,
) -> Result<(), DatabaseMigrationError> {
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(false)
        .read_only(true)
        .foreign_keys(true)
        .busy_timeout(Duration::from_secs(5));
    let mut connection = SqliteConnection::connect_with(&options).await?;
    let history = read_schema_history(&mut connection).await?;
    validate_schema_history_for(&history, migrations, current_version, true)?;
    if history != expected_history {
        return Err(sqlx::Error::Protocol(
            "database migration backup history differs from its source".into(),
        )
        .into());
    }
    verify_database(&mut connection).await?;
    Ok(())
}

struct PendingSnapshot {
    directory: PathBuf,
    snapshot: PathBuf,
    final_path: PathBuf,
}

impl PendingSnapshot {
    fn create(final_path: &Path) -> Result<Self, DatabaseMigrationError> {
        let parent = final_path
            .parent()
            .ok_or(DatabaseMigrationError::InvalidBackupPath)?;
        for attempt in 0_u16..128 {
            let directory = parent.join(format!(
                ".marginalis-database-migration-{}-{attempt}",
                std::process::id()
            ));
            match DirBuilder::new()
                .mode(PRIVATE_DIRECTORY_MODE)
                .create(&directory)
            {
                Ok(()) => {
                    return Ok(Self {
                        snapshot: directory.join("database.sqlite3"),
                        directory,
                        final_path: final_path.to_owned(),
                    });
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(DatabaseMigrationError::BackupIo(error)),
            }
        }
        Err(DatabaseMigrationError::BackupIo(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "could not allocate a private migration directory",
        )))
    }

    fn snapshot_path(&self) -> &Path {
        &self.snapshot
    }

    fn publish(self) -> Result<(), DatabaseMigrationError> {
        File::open(&self.snapshot)
            .and_then(|file| file.sync_all())
            .map_err(DatabaseMigrationError::BackupIo)?;
        std::fs::hard_link(&self.snapshot, &self.final_path).map_err(|error| {
            if error.kind() == io::ErrorKind::AlreadyExists {
                DatabaseMigrationError::BackupAlreadyExists
            } else {
                DatabaseMigrationError::BackupIo(error)
            }
        })?;
        sync_directory(
            self.final_path
                .parent()
                .ok_or(DatabaseMigrationError::InvalidBackupPath)?,
        )?;
        std::fs::remove_file(&self.snapshot).map_err(DatabaseMigrationError::BackupIo)?;
        std::fs::remove_dir(&self.directory).map_err(DatabaseMigrationError::BackupIo)?;
        sync_directory(
            self.final_path
                .parent()
                .ok_or(DatabaseMigrationError::InvalidBackupPath)?,
        )?;
        Ok(())
    }
}

impl Drop for PendingSnapshot {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.snapshot);
        let _ = std::fs::remove_dir(&self.directory);
    }
}

fn sync_directory(path: &Path) -> Result<(), DatabaseMigrationError> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(DatabaseMigrationError::BackupIo)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::Executor as _;
    use std::fs;

    const SCHEMA_22: &str = include_str!("tests/schema_22.sql");

    const TEST_MIGRATIONS: &[Migration] = &[
        Migration {
            from: 22,
            to: 23,
            sql: "ALTER TABLE migration_fixture ADD COLUMN migrated_value TEXT NOT NULL DEFAULT 'first';",
            rebuild_current_schema: false,
            copy_sql: "",
        },
        Migration {
            from: 23,
            to: 24,
            sql: "UPDATE migration_fixture SET migrated_value = 'second';",
            rebuild_current_schema: false,
            copy_sql: "",
        },
    ];

    fn test_paths(name: &str) -> (PathBuf, PathBuf, PathBuf) {
        let directory = std::env::temp_dir().join(format!(
            "marginalis-database-migration-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir(&directory).expect("test directory");
        let database = directory.join("source.sqlite3");
        let backup = directory.join("backup.sqlite3");
        (directory, database, backup)
    }

    async fn create_schema_22_fixture(path: &Path) {
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .foreign_keys(true);
        let mut connection = SqliteConnection::connect_with(&options)
            .await
            .expect("fixture database");
        connection
            .execute(
                "CREATE TABLE schema_migrations (version INTEGER PRIMARY KEY NOT NULL) STRICT;
                 INSERT INTO schema_migrations (version) VALUES (22);
                 CREATE TABLE migration_fixture (
                     fixture_id INTEGER PRIMARY KEY NOT NULL,
                     secret_value TEXT NOT NULL
                 ) STRICT;
                 INSERT INTO migration_fixture (fixture_id, secret_value) VALUES (1, 'preserved');",
            )
            .await
            .expect("fixture schema");
    }

    #[tokio::test]
    async fn applies_all_pending_migrations_after_publishing_a_private_backup() {
        let (directory, database, backup) = test_paths("success");
        create_schema_22_fixture(&database).await;
        let database_url = format!("sqlite:{}", database.display());

        let report = migrate_database_with(&database_url, &backup, TEST_MIGRATIONS, 24)
            .await
            .expect("migration succeeds");

        assert_eq!(report.from_version, 22);
        assert_eq!(report.to_version, 24);
        assert_eq!(report.applied_migrations, 2);
        assert_eq!(report.backup_path.as_deref(), Some(backup.as_path()));
        assert_eq!(
            fs::metadata(&backup)
                .expect("backup metadata")
                .permissions()
                .mode()
                & 0o777,
            PRIVATE_FILE_MODE
        );

        let mut migrated = SqliteConnection::connect(&database_url)
            .await
            .expect("migrated database");
        assert_eq!(
            read_schema_history(&mut migrated).await.unwrap(),
            [22, 23, 24]
        );
        assert_eq!(
            sqlx::query_scalar::<_, String>(
                "SELECT migrated_value FROM migration_fixture WHERE fixture_id = 1"
            )
            .fetch_one(&mut migrated)
            .await
            .unwrap(),
            "second"
        );

        let backup_url = format!("sqlite:{}?mode=ro", backup.display());
        let mut preserved = SqliteConnection::connect(&backup_url)
            .await
            .expect("backup database");
        assert_eq!(read_schema_history(&mut preserved).await.unwrap(), [22]);
        assert_eq!(
            sqlx::query_scalar::<_, String>(
                "SELECT secret_value FROM migration_fixture WHERE fixture_id = 1"
            )
            .fetch_one(&mut preserved)
            .await
            .unwrap(),
            "preserved"
        );
        fs::remove_dir_all(directory).expect("remove test directory");
    }

    #[tokio::test]
    async fn current_schema_is_a_no_op_without_creating_a_backup() {
        let (directory, database, backup) = test_paths("current");
        create_schema_22_fixture(&database).await;
        let database_url = format!("sqlite:{}", database.display());

        let report = migrate_database_with(&database_url, &backup, &[], 22)
            .await
            .expect("current schema");

        assert_eq!(report.applied_migrations, 0);
        assert_eq!(report.backup_path, None);
        assert!(!backup.exists());
        fs::remove_dir_all(directory).expect("remove test directory");
    }

    #[tokio::test]
    async fn failed_migration_rolls_back_but_keeps_the_verified_backup() {
        let (directory, database, backup) = test_paths("rollback");
        create_schema_22_fixture(&database).await;
        let database_url = format!("sqlite:{}", database.display());
        let failing = [Migration {
            from: 22,
            to: 23,
            sql: "ALTER TABLE migration_fixture ADD COLUMN temporary_value TEXT; SELECT missing FROM nowhere;",
            rebuild_current_schema: false,
            copy_sql: "",
        }];

        migrate_database_with(&database_url, &backup, &failing, 23)
            .await
            .expect_err("migration fails");

        assert!(backup.is_file());
        let mut connection = SqliteConnection::connect(&database_url)
            .await
            .expect("source database");
        assert_eq!(read_schema_history(&mut connection).await.unwrap(), [22]);
        let columns = sqlx::query_scalar::<_, String>(
            "SELECT name FROM pragma_table_info('migration_fixture') ORDER BY cid",
        )
        .fetch_all(&mut connection)
        .await
        .unwrap();
        assert_eq!(columns, ["fixture_id", "secret_value"]);
        connection.close().await.unwrap();

        let retry_backup = directory.join("retry-backup.sqlite3");
        let retried = migrate_database_with(&database_url, &retry_backup, TEST_MIGRATIONS, 24)
            .await
            .expect("migration can be retried with a new backup path");
        assert_eq!(retried.applied_migrations, 2);
        assert!(retry_backup.is_file());
        fs::remove_dir_all(directory).expect("remove test directory");
    }

    #[tokio::test]
    async fn schema_22_data_is_rebuilt_as_the_same_schema_created_fresh_at_23() {
        let (directory, database, backup) = test_paths("schema-22-to-23");
        let options = SqliteConnectOptions::new()
            .filename(&database)
            .create_if_missing(true)
            .foreign_keys(true);
        let mut connection = SqliteConnection::connect_with(&options)
            .await
            .expect("schema 22 connection");
        sqlx::query("CREATE TABLE schema_migrations (version INTEGER PRIMARY KEY NOT NULL) STRICT")
            .execute(&mut connection)
            .await
            .expect("schema history");
        sqlx::query("INSERT INTO schema_migrations (version) VALUES (22)")
            .execute(&mut connection)
            .await
            .expect("baseline history");
        sqlx::raw_sql(SCHEMA_22)
            .execute(&mut connection)
            .await
            .expect("schema 22");
        sqlx::raw_sql(
            r#"
            INSERT INTO notes (
                note_id, creator_issuer, creator_subject, title, source, tags_json,
                created_at_ms, updated_at_ms, revision, deleted_at_ms, created_via,
                review_tracking_known, reviewed_revision, reviewed_at_ms,
                reviewer_issuer, reviewer_subject
            ) VALUES (
                'note-1', 'https://id.example.test', 'alice', '題名', '= 題名', '[]',
                1, 2, 1, NULL, 'web', 1, 1, 2,
                'https://id.example.test', 'alice'
            );
            INSERT INTO note_acl (note_id, issuer, subject, permission)
            VALUES ('note-1', 'https://id.example.test', 'bob', 'read');
            INSERT INTO bibliography_items (
                item_id, owner_issuer, owner_subject, citation_key, csl_json,
                created_at_ms, updated_at_ms, revision
            ) VALUES (
                'item-1', 'https://id.example.test', 'alice', 'smith2026',
                '{"id":"smith2026","type":"book"}', 1, 2, 1
            );
            INSERT INTO math_macro_settings (owner_issuer, owner_subject, macros_json, revision)
            VALUES ('https://id.example.test', 'alice', '[]', 1);
            INSERT INTO web_sessions (
                session_id_hash, csrf_token_hash, issuer, subject, issued_at_ms,
                last_seen_at_ms, idle_expires_at_ms, absolute_expires_at_ms, revoked_at_ms
            ) VALUES (X'01', X'02', 'https://id.example.test', 'alice', 1, 2, 3, 4, NULL);
            INSERT INTO mcp_clients (
                client_id, display_name, redirect_uris_json, registration_method, registered_at_ms
            ) VALUES ('client-1', 'Client', '[]', 'dynamic', 1);
            INSERT INTO mcp_access_tokens (
                token_hash, client_id, resource_uri, issuer, subject, scopes,
                expires_at_ms, revoked_at_ms, last_used_at_ms, token_family_id
            ) VALUES (
                X'03', 'client-1', 'https://app.example.test/mcp',
                'https://id.example.test', 'alice', 'notes:read', 10, NULL, NULL,
                zeroblob(32)
            );
            INSERT INTO webhook_subscriptions (
                subscription_id, owner_issuer, owner_subject, url, secret,
                event_kinds_json, state, disabled_reason, created_at_ms, updated_at_ms, revision
            ) VALUES (
                'subscription-1', 'https://id.example.test', 'alice',
                'https://receiver.example.test/hook', 'secret', '["note.created"]',
                'active', NULL, 1, 1, 1
            );
            INSERT INTO webhook_outbox_events (
                event_sequence, event_id, owner_issuer, owner_subject, event_kind,
                target_id, revision, occurred_at_ms
            ) VALUES (
                20, 'event-1', 'https://id.example.test', 'alice',
                'note.created', 'note-1', 1, 2
            );
            UPDATE webhook_deliveries SET state = 'delivered', attempt_count = 1,
                last_attempted_at_ms = 3 WHERE event_sequence = 20;
            "#,
        )
        .execute(&mut connection)
        .await
        .expect("representative schema 22 data");
        connection.close().await.expect("close schema 22");

        let report = migrate_database(&format!("sqlite:{}", database.display()), &backup)
            .await
            .expect("migrate schema 22");
        assert_eq!((report.from_version, report.to_version), (22, 23));

        let mut migrated = SqliteConnection::connect_with(
            &SqliteConnectOptions::new()
                .filename(&database)
                .create_if_missing(false)
                .foreign_keys(true),
        )
        .await
        .expect("migrated database");
        let identities = sqlx::query_as::<_, (i64, String, String)>(
            "SELECT principal_id, issuer, subject FROM principal_identities ORDER BY principal_id",
        )
        .fetch_all(&mut migrated)
        .await
        .expect("principal identities");
        assert_eq!(
            identities,
            vec![
                (1, "https://id.example.test".into(), "alice".into()),
                (2, "https://id.example.test".into(), "bob".into()),
            ]
        );
        let note_owner = sqlx::query_as::<_, (i64, Option<i64>)>(
            "SELECT creator_principal_id, reviewer_principal_id FROM notes WHERE note_id = 'note-1'",
        )
        .fetch_one(&mut migrated)
        .await
        .expect("migrated note");
        assert_eq!(note_owner, (1, Some(1)));
        let session_identity = sqlx::query_as::<_, (i64, i64)>(
            "SELECT principal_id, authenticated_identity_id FROM web_sessions",
        )
        .fetch_one(&mut migrated)
        .await
        .expect("migrated session");
        assert_eq!(session_identity, (1, 1));
        let delivery = sqlx::query_as::<_, (String, i64, String, i64)>(
            "SELECT subscription_id, event_sequence, state, attempt_count FROM webhook_deliveries",
        )
        .fetch_one(&mut migrated)
        .await
        .expect("migrated delivery");
        assert_eq!(
            delivery,
            ("subscription-1".into(), 20, "delivered".into(), 1)
        );

        let fresh_path = directory.join("fresh.sqlite3");
        let mut fresh = SqliteConnection::connect_with(
            &SqliteConnectOptions::new()
                .filename(&fresh_path)
                .create_if_missing(true)
                .foreign_keys(true),
        )
        .await
        .expect("fresh connection");
        sqlx::query("CREATE TABLE schema_migrations (version INTEGER PRIMARY KEY NOT NULL) STRICT")
            .execute(&mut fresh)
            .await
            .expect("fresh history table");
        sqlx::raw_sql(INITIAL_SCHEMA)
            .execute(&mut fresh)
            .await
            .expect("fresh schema");
        let schema_query = "SELECT type, name, sql FROM sqlite_schema \
                            WHERE name NOT LIKE 'sqlite_%' ORDER BY type, name";
        let migrated_schema = sqlx::query_as::<_, (String, String, Option<String>)>(schema_query)
            .fetch_all(&mut migrated)
            .await
            .expect("migrated schema objects");
        let fresh_schema = sqlx::query_as::<_, (String, String, Option<String>)>(schema_query)
            .fetch_all(&mut fresh)
            .await
            .expect("fresh schema objects");
        assert_eq!(migrated_schema, fresh_schema);

        migrated.close().await.expect("close migrated database");
        fresh.close().await.expect("close fresh database");
        fs::remove_dir_all(directory).expect("remove test directory");
    }

    #[tokio::test]
    async fn existing_backup_is_never_overwritten_and_source_is_unchanged() {
        let (directory, database, backup) = test_paths("existing-backup");
        create_schema_22_fixture(&database).await;
        fs::write(&backup, b"existing backup").expect("existing backup");
        let database_url = format!("sqlite:{}", database.display());

        let error = migrate_database_with(&database_url, &backup, TEST_MIGRATIONS, 24)
            .await
            .expect_err("existing backup is rejected");

        assert!(matches!(error, DatabaseMigrationError::BackupAlreadyExists));
        assert_eq!(fs::read(&backup).unwrap(), b"existing backup");
        let mut connection = SqliteConnection::connect(&database_url).await.unwrap();
        assert_eq!(read_schema_history(&mut connection).await.unwrap(), [22]);
        fs::remove_dir_all(directory).expect("remove test directory");
    }

    #[tokio::test]
    async fn rejects_non_consecutive_history_before_creating_a_backup() {
        let (directory, database, backup) = test_paths("history");
        create_schema_22_fixture(&database).await;
        let database_url = format!("sqlite:{}", database.display());
        let mut connection = SqliteConnection::connect(&database_url).await.unwrap();
        connection
            .execute("INSERT INTO schema_migrations (version) VALUES (24)")
            .await
            .unwrap();
        connection.close().await.unwrap();

        migrate_database_with(&database_url, &backup, TEST_MIGRATIONS, 24)
            .await
            .expect_err("history gap is rejected");

        assert!(!backup.exists());
        fs::remove_dir_all(directory).expect("remove test directory");
    }
}
