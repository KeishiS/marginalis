use std::time::Duration;

use oidc_browser_login::{
    UnixMillis as SharedUnixMillis,
    session::{Principal, TokenDigest, WebSessionRecord, WebSessionStore},
};

use super::*;

fn session_record(
    session: &str,
    csrf: &str,
    subject: &str,
    idle_expires_at_ms: i64,
    absolute_expires_at_ms: i64,
) -> WebSessionRecord {
    WebSessionRecord {
        session_digest: TokenDigest::of(session),
        csrf_digest: TokenDigest::of(csrf),
        principal: Principal::new(ISSUER.into(), subject.into()).expect("valid test principal"),
        idle_expires_at: SharedUnixMillis::new(idle_expires_at_ms),
        absolute_expires_at: SharedUnixMillis::new(absolute_expires_at_ms),
    }
}

#[tokio::test]
async fn sessions_retain_the_validated_identity() {
    let database = database().await;
    let store = database.web_session_store();
    store
        .issue(
            session_record("session-token", "csrf-token", "alice", 1_000, 2_000),
            SharedUnixMillis::new(100),
        )
        .await
        .expect("issue session");
    let stored_csrf = store
        .csrf_digest(TokenDigest::of("session-token"))
        .await
        .expect("csrf query")
        .expect("stored csrf digest");
    assert!(stored_csrf.constant_time_eq(&TokenDigest::of("csrf-token")));
    assert!(!stored_csrf.constant_time_eq(&TokenDigest::of("wrong")));
    let authenticated = store
        .lookup_and_extend(
            TokenDigest::of("session-token"),
            SharedUnixMillis::new(200),
            Duration::from_millis(900),
        )
        .await
        .expect("lookup")
        .expect("active session");
    assert_eq!(authenticated.principal.subject(), "alice");
    assert_eq!(authenticated.idle_expires_at, SharedUnixMillis::new(1_100));
    assert_eq!(
        store
            .lookup_and_extend(
                TokenDigest::of("session-token"),
                SharedUnixMillis::new(1_050),
                Duration::from_millis(900),
            )
            .await
            .expect("sliding lookup")
            .expect("activity extends the session")
            .idle_expires_at,
        SharedUnixMillis::new(1_950)
    );
    assert_eq!(
        store
            .lookup_and_extend(
                TokenDigest::of("session-token"),
                SharedUnixMillis::new(1_900),
                Duration::from_millis(900),
            )
            .await
            .expect("absolute cap lookup")
            .expect("session remains active before the absolute limit")
            .idle_expires_at,
        SharedUnixMillis::new(2_000)
    );
    assert!(
        store
            .lookup_and_extend(
                TokenDigest::of("session-token"),
                SharedUnixMillis::new(2_000),
                Duration::from_millis(900),
            )
            .await
            .expect("expired lookup")
            .is_none()
    );
    store
        .issue(
            session_record(
                "replacement-session",
                "replacement-csrf",
                "alice",
                3_000,
                4_000,
            ),
            SharedUnixMillis::new(2_100),
        )
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

/// SQLiteのsession storeが共有crateの`WebSessionStore`契約と交換可能なことを確かめる。
#[tokio::test]
async fn web_session_store_satisfies_the_shared_contract() {
    oidc_browser_login_testkit::check_web_session_store_contract(|| async {
        database().await.web_session_store()
    })
    .await;
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
    let store = database.web_session_store();
    store
        .issue(
            session_record(
                "shared-session-token",
                "shared-csrf-token",
                "alice",
                1_000,
                10_000,
            ),
            SharedUnixMillis::new(100),
        )
        .await
        .expect("issue shared session");

    const LOOKUP_COUNT: usize = 16;
    let barrier = Arc::new(Barrier::new(LOOKUP_COUNT));
    let mut lookups = Vec::with_capacity(LOOKUP_COUNT);
    for _ in 0..LOOKUP_COUNT {
        let store = store.clone();
        let barrier = Arc::clone(&barrier);
        lookups.push(tokio::spawn(async move {
            barrier.wait().await;
            store
                .lookup_and_extend(
                    TokenDigest::of("shared-session-token"),
                    SharedUnixMillis::new(200),
                    Duration::from_millis(1_000),
                )
                .await
        }));
    }
    for lookup in lookups {
        let session = lookup
            .await
            .expect("lookup task completes")
            .expect("concurrent lookup succeeds")
            .expect("session remains active");
        assert_eq!(session.idle_expires_at, SharedUnixMillis::new(1_200));
    }

    database.pool.close().await;
    std::fs::remove_file(&database_path).expect("remove temporary database");
    let _ = std::fs::remove_file(database_path.with_extension("sqlite-shm"));
    let _ = std::fs::remove_file(database_path.with_extension("sqlite-wal"));
}

#[tokio::test]
async fn explicit_auth_cleanup_removes_expired_rows_without_new_issuance() {
    let database = database().await;
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

#[tokio::test]
async fn issuing_login_attempt_reclaims_expired_capacity_before_enforcing_the_limit() {
    let database = database().await;
    let mut transaction = database.pool.begin().await.expect("begin transaction");
    for index in 0_i64..1_024 {
        sqlx::query(
            "INSERT INTO oidc_login_attempts
             (state_hash, nonce, pkce_verifier, expires_at_ms)
             VALUES (?, ?, ?, ?)",
        )
        .bind(index.to_be_bytes().to_vec())
        .bind(format!("nonce-{index}"))
        .bind(format!("verifier-{index}"))
        .bind(1_000_i64)
        .execute(&mut *transaction)
        .await
        .expect("insert pending attempt");
    }
    transaction.commit().await.expect("commit attempts");

    let attempts = database.oidc_login_attempt_store();
    let fresh_attempt = OidcLoginAttempt {
        state: "fresh-state".into(),
        nonce: "fresh-nonce".into(),
        pkce_verifier: "fresh-verifier".into(),
        expires_at: UnixMillis::new(2_000),
    };
    assert!(
        attempts
            .issue(fresh_attempt.clone(), UnixMillis::new(999))
            .await
            .is_err(),
        "有効なattemptが上限に達している間は発行を拒否します"
    );
    attempts
        .issue(fresh_attempt, UnixMillis::new(1_000))
        .await
        .expect("期限切れattemptを削除して発行できる");

    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM oidc_login_attempts")
            .fetch_one(&database.pool)
            .await
            .expect("attempt count"),
        1
    );
}

/// SQLiteのlogin attempt storeが、共有crateの`LoginAttemptStore`契約と交換可能なことを
/// testkitの契約試験で確かめる。wrapperはmarginalis-auth-oidcのport写像と同じ変換を行う。
#[tokio::test]
async fn oidc_login_attempt_store_satisfies_the_shared_contract() {
    struct SharedStore<A>(A);

    impl<A: OidcLoginAttemptStore> oidc_browser_login::LoginAttemptStore for SharedStore<A> {
        type Error = A::Error;

        async fn issue(
            &self,
            attempt: oidc_browser_login::LoginAttempt,
            now: oidc_browser_login::UnixMillis,
        ) -> Result<(), Self::Error> {
            self.0
                .issue(
                    OidcLoginAttempt {
                        state: attempt.state,
                        nonce: attempt.nonce,
                        pkce_verifier: attempt.pkce_verifier,
                        expires_at: UnixMillis::new(attempt.expires_at.get()),
                    },
                    UnixMillis::new(now.get()),
                )
                .await
        }

        async fn consume(
            &self,
            state: String,
            now: oidc_browser_login::UnixMillis,
        ) -> Result<Option<oidc_browser_login::LoginAttempt>, Self::Error> {
            Ok(self
                .0
                .consume(state, UnixMillis::new(now.get()))
                .await?
                .map(|attempt| oidc_browser_login::LoginAttempt {
                    state: attempt.state,
                    nonce: attempt.nonce,
                    pkce_verifier: attempt.pkce_verifier,
                    expires_at: oidc_browser_login::UnixMillis::new(attempt.expires_at.get()),
                }))
        }
    }

    oidc_browser_login_testkit::check_login_attempt_store_contract(|| async {
        SharedStore(database().await.oidc_login_attempt_store())
    })
    .await;
}
