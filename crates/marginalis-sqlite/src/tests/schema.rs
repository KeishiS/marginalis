use super::*;

#[tokio::test]
async fn initialization_rejects_a_database_with_unknown_tables() {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("database");
    sqlx::query("CREATE TABLE unknown_notes (note_id TEXT PRIMARY KEY NOT NULL) STRICT")
        .execute(&pool)
        .await
        .expect("unknown table");

    let error = initialize_or_validate_schema(&pool)
        .await
        .expect_err("non-empty database must be rejected");
    assert!(
        error
            .to_string()
            .contains("initialization requires an empty database")
    );
    assert!(
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM sqlite_schema WHERE type = 'table' AND name = 'notes'"
        )
        .fetch_one(&pool)
        .await
        .expect("schema query")
            == 0
    );
}

#[tokio::test]
async fn initialization_rejects_the_previous_schema_version() {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("database");
    sqlx::query("CREATE TABLE schema_migrations (version INTEGER PRIMARY KEY NOT NULL) STRICT")
        .execute(&pool)
        .await
        .expect("migration table");
    sqlx::query("INSERT INTO schema_migrations (version) VALUES (11)")
        .execute(&pool)
        .await
        .expect("old version");

    let error = initialize_or_validate_schema(&pool)
        .await
        .expect_err("old schema must be rejected");
    assert!(
        error
            .to_string()
            .contains("unsupported database schema version 11; expected 12")
    );
}
