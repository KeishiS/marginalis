use super::*;

#[tokio::test]
async fn sessions_retain_the_validated_identity() {
    let database = SqliteDatabase::connect("sqlite::memory:")
        .await
        .expect("schema initialization succeeds");
    let session = WebSession {
        session_id: "session-token".into(),
        csrf_token: "csrf-token".into(),
        actor: actor("https://id.example.test", "alice"),
        idle_expires_at: UnixMillis::new(1_000),
        absolute_expires_at: UnixMillis::new(2_000),
    };
    database
        .issue_web_session(&session, UnixMillis::new(100))
        .await
        .expect("issue session");
    assert!(
        database
            .validate_web_session_csrf("session-token", "csrf-token")
            .await
            .expect("csrf query")
    );
    assert!(
        !database
            .validate_web_session_csrf("session-token", "wrong")
            .await
            .expect("csrf query")
    );
    let authenticated = database
        .lookup_web_session("session-token", UnixMillis::new(200), 900)
        .await
        .expect("lookup")
        .expect("active session");
    assert_eq!(authenticated.idle_expires_at, UnixMillis::new(1_100));
    assert_eq!(
        database
            .lookup_web_session("session-token", UnixMillis::new(1_050), 900)
            .await
            .expect("sliding lookup")
            .expect("activity extends the session")
            .idle_expires_at,
        UnixMillis::new(1_950)
    );
    assert_eq!(
        database
            .lookup_web_session("session-token", UnixMillis::new(1_900), 900)
            .await
            .expect("absolute cap lookup")
            .expect("session remains active before the absolute limit")
            .idle_expires_at,
        UnixMillis::new(2_000)
    );
    assert_eq!(
        database
            .lookup_web_session("session-token", UnixMillis::new(2_000), 900)
            .await,
        Ok(None)
    );
    let replacement = WebSession {
        session_id: "replacement-session".into(),
        csrf_token: "replacement-csrf".into(),
        actor: session.actor,
        idle_expires_at: UnixMillis::new(3_000),
        absolute_expires_at: UnixMillis::new(4_000),
    };
    database
        .issue_web_session(&replacement, UnixMillis::new(2_100))
        .await
        .expect("issue replacement session");
    let counts = database
        .purge_expired_auth_state(UnixMillis::new(2_100), UnixMillis::new(0))
        .await
        .expect("explicit session cleanup");
    assert_eq!(counts.web_sessions, 1);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM web_sessions")
            .fetch_one(&database.pool)
            .await
            .expect("session count"),
        1
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_session_lookups_extend_one_session_without_snapshot_failures() {
    use std::{
        sync::Arc,
        time::{SystemTime, UNIX_EPOCH},
    };

    use tokio::sync::Barrier;

    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock is after the Unix epoch")
        .as_nanos();
    let database_path = std::env::temp_dir().join(format!(
        "marginalis-session-concurrency-{}-{unique}.sqlite",
        std::process::id()
    ));
    let database_url = format!("sqlite://{}?mode=rwc", database_path.display());
    let database = SqliteDatabase::connect(&database_url)
        .await
        .expect("schema initialization succeeds");
    database
        .issue_web_session(
            &WebSession {
                session_id: "shared-session-token".into(),
                csrf_token: "shared-csrf-token".into(),
                actor: actor("https://id.example.test", "alice"),
                idle_expires_at: UnixMillis::new(1_000),
                absolute_expires_at: UnixMillis::new(10_000),
            },
            UnixMillis::new(100),
        )
        .await
        .expect("issue shared session");

    const LOOKUP_COUNT: usize = 16;
    let barrier = Arc::new(Barrier::new(LOOKUP_COUNT));
    let mut lookups = Vec::with_capacity(LOOKUP_COUNT);
    for _ in 0..LOOKUP_COUNT {
        let database = database.clone();
        let barrier = Arc::clone(&barrier);
        lookups.push(tokio::spawn(async move {
            barrier.wait().await;
            database
                .lookup_web_session("shared-session-token", UnixMillis::new(200), 1_000)
                .await
        }));
    }
    for lookup in lookups {
        let session = lookup
            .await
            .expect("lookup task completes")
            .expect("concurrent lookup succeeds")
            .expect("session remains active");
        assert_eq!(session.idle_expires_at, UnixMillis::new(1_200));
    }

    database.pool.close().await;
    std::fs::remove_file(&database_path).expect("remove temporary database");
    let _ = std::fs::remove_file(database_path.with_extension("sqlite-shm"));
    let _ = std::fs::remove_file(database_path.with_extension("sqlite-wal"));
}

#[tokio::test]
async fn explicit_auth_cleanup_removes_expired_rows_without_new_issuance() {
    let database = SqliteDatabase::connect("sqlite::memory:")
        .await
        .expect("schema initialization succeeds");
    let attempts = database.oidc_login_attempt_store();
    attempts
        .issue(
            OidcLoginAttempt {
                state: "expired-state".into(),
                nonce: "expired-nonce".into(),
                pkce_verifier: "expired-verifier".into(),
                expires_at: UnixMillis::new(1_000),
            },
            UnixMillis::new(100),
        )
        .await
        .expect("first attempt");
    attempts
        .issue(
            OidcLoginAttempt {
                state: "active-state".into(),
                nonce: "active-nonce".into(),
                pkce_verifier: "active-verifier".into(),
                expires_at: UnixMillis::new(2_000),
            },
            UnixMillis::new(100),
        )
        .await
        .expect("active attempt");
    attempts
        .issue(
            OidcLoginAttempt {
                state: "consumed-expired-state".into(),
                nonce: "expired-nonce".into(),
                pkce_verifier: "expired-verifier".into(),
                expires_at: UnixMillis::new(1_000),
            },
            UnixMillis::new(100),
        )
        .await
        .expect("expired attempt to consume");
    assert_eq!(
        attempts
            .consume("consumed-expired-state".into(), UnixMillis::new(1_000))
            .await
            .expect("consume expired attempt"),
        None
    );
    assert_eq!(
        attempts
            .consume("consumed-expired-state".into(), UnixMillis::new(1_000))
            .await
            .expect("replay consumed attempt"),
        None
    );
    let counts = database
        .purge_expired_auth_state(UnixMillis::new(1_000), UnixMillis::new(0))
        .await
        .expect("explicit cleanup");
    assert_eq!(counts.oidc_login_attempts, 1);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM oidc_login_attempts")
            .fetch_one(&database.pool)
            .await
            .expect("attempt count"),
        1
    );
}
