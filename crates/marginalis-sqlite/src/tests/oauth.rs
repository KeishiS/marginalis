use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use marginalis_application::{
    Clock, McpAuthorizationRequest, McpClientMetadataResolver, McpClientRegistrationMethod,
    McpOAuthApplication, McpOAuthRepositoryError, McpRegisteredOAuthClient, Random,
};
use marginalis_domain::EntityId as ApplicationEntityId;
use sha2::{Digest, Sha256};

use super::*;

/// 決定的な時刻とtokenを供給する試験用の実装。
struct FixedClock(i64);

impl Clock for FixedClock {
    fn now(&self) -> UnixMillis {
        UnixMillis::new(self.0)
    }
}

struct SequentialRandom(std::sync::Mutex<u32>);

impl Random for SequentialRandom {
    fn uuid_v7(&self) -> ApplicationEntityId {
        EntityId::from_str("018f0000-0000-7000-8000-000000000000").expect("test entity ID")
    }

    fn opaque_token(&self) -> String {
        let mut counter = self.0.lock().expect("test random counter");
        *counter += 1;
        format!("opaque-token-{counter}")
    }
}

/// Client ID Metadata Documentの取得結果だけを差し替える。永続化はSQLiteの実装を使う。
struct StaticMetadataResolver {
    client: McpOAuthClient,
    calls: AtomicUsize,
}

#[async_trait::async_trait]
impl McpClientMetadataResolver for StaticMetadataResolver {
    async fn resolve(
        &self,
        client_id: &str,
    ) -> Result<Option<McpOAuthClient>, McpOAuthRepositoryError> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        Ok((client_id == self.client.client_id).then(|| self.client.clone()))
    }
}

fn registered_client(
    client: &McpOAuthClient,
    registration_method: McpClientRegistrationMethod,
) -> McpRegisteredOAuthClient {
    McpRegisteredOAuthClient {
        client: client.clone(),
        registration_method,
    }
}

/// 事前登録のないClient ID Metadata Document clientでも、認可からMCP利用まで通ることを確認する。
///
/// `mcp_authorization_codes.client_id`は`mcp_clients`への外部keyを持つため、解決しただけで
/// 登録していないclientでは認可code発行が失敗していた。applicationとSQLiteを実際に結線し、
/// 取得だけを差し替えて経路全体を確認する。
#[tokio::test]
async fn client_id_metadata_document_clients_complete_the_authorization_flow() {
    let database = SqliteDatabase::connect("sqlite::memory:")
        .await
        .expect("database");
    let client = McpOAuthClient {
        client_id: "https://client.example.test/oauth/metadata.json".into(),
        display_name: "Metadata document client".into(),
        redirect_uris: vec!["https://client.example.test/callback".into()],
    };
    let resource_uri = "https://notes.example.test/mcp".to_owned();
    let resolver = Arc::new(StaticMetadataResolver {
        client: client.clone(),
        calls: AtomicUsize::new(0),
    });
    let application = McpOAuthApplication::new(
        Arc::new(database.clone()),
        Arc::new(FixedClock(1_000)),
        Arc::new(SequentialRandom(std::sync::Mutex::new(0))),
        resource_uri.clone(),
    )
    .with_client_metadata_resolver(resolver.clone());

    let verifier = "a".repeat(43);
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let validated = application
        .validate_authorization_request(&McpAuthorizationRequest {
            client_id: client.client_id.clone(),
            redirect_uri: client.redirect_uris[0].clone(),
            resource_uri: resource_uri.clone(),
            scopes: vec!["notes:read".into(), "notes:write".into()],
            code_challenge: challenge,
        })
        .await
        .expect("validated authorization request");
    assert_eq!(validated.client, client);
    assert_eq!(
        validated.registration_method,
        McpClientRegistrationMethod::MetadataDocument
    );

    let code = application
        .authorize(actor("https://id.example.test", "alice"), validated)
        .await
        .expect("authorization code");
    assert_eq!(
        database
            .registered_mcp_client(&client.client_id)
            .await
            .expect("lookup"),
        Some(McpRegisteredOAuthClient {
            client: client.clone(),
            registration_method: McpClientRegistrationMethod::MetadataDocument,
        }),
        "同意した時点でclientを登録する"
    );

    application
        .validate_authorization_request(&McpAuthorizationRequest {
            client_id: client.client_id.clone(),
            redirect_uri: client.redirect_uris[0].clone(),
            resource_uri: resource_uri.clone(),
            scopes: vec!["notes:read".into()],
            code_challenge: URL_SAFE_NO_PAD.encode(Sha256::digest(b"b".repeat(43))),
        })
        .await
        .expect("revalidated authorization request");
    assert_eq!(
        resolver.calls.load(Ordering::Relaxed),
        2,
        "永続化したmetadata document clientも取得し直す"
    );

    let pair = application
        .exchange_authorization_code(
            code,
            client.client_id.clone(),
            Some(client.redirect_uris[0].clone()),
            resource_uri.clone(),
            verifier,
        )
        .await
        .expect("token pair");
    assert_eq!(pair.scope, "notes:read notes:write");

    let authenticated = application
        .authenticate(&pair.access_token, &resource_uri)
        .await
        .expect("authentication")
        .expect("authenticated actor");
    assert_eq!(authenticated.actor.subject(), "alice");
    assert_eq!(authenticated.scopes, vec!["notes:read", "notes:write"]);
}

#[tokio::test]
async fn rfc7009_revocation_revokes_the_whole_token_family() {
    let database = SqliteDatabase::connect("sqlite::memory:")
        .await
        .expect("database");
    let client = McpOAuthClient {
        client_id: "public-client".into(),
        display_name: "Public client".into(),
        redirect_uris: vec!["https://client.example/callback".into()],
    };
    database
        .upsert_mcp_client(&client, UnixMillis::new(0))
        .await
        .expect("client");
    let grant = McpAuthorizationGrant {
        actor: actor("https://id.example", "alice"),
        client_id: client.client_id.clone(),
        redirect_uri: client.redirect_uris[0].clone(),
        resource_uri: "https://notes.example/mcp".into(),
        scopes: vec!["notes:read".into()],
    };
    database
        .issue_mcp_authorization_code(
            "code",
            &registered_client(&client, McpClientRegistrationMethod::Dynamic),
            &grant,
            "challenge",
            UnixMillis::new(100),
            UnixMillis::new(0),
        )
        .await
        .expect("code");
    database
        .exchange_mcp_authorization_code(
            McpAuthorizationCodeExchange {
                code: "code".into(),
                client_id: client.client_id.clone(),
                redirect_uri: Some(grant.redirect_uri.clone()),
                resource_uri: grant.resource_uri.clone(),
                code_challenge: "challenge".into(),
                access_token: "access".into(),
                refresh_token: "refresh".into(),
                access_expires_at: UnixMillis::new(500),
                refresh_expires_at: UnixMillis::new(900),
            },
            UnixMillis::new(1),
        )
        .await
        .expect("token pair")
        .expect("grant");

    database
        .revoke_mcp_token("refresh", &client.client_id, UnixMillis::new(2))
        .await
        .expect("revocation");
    assert!(
        database
            .authenticate_mcp_access_token("access", &grant.resource_uri, UnixMillis::new(3))
            .await
            .expect("authentication")
            .is_none()
    );
    database
        .revoke_mcp_token("unknown", &client.client_id, UnixMillis::new(4))
        .await
        .expect("unknown token is indistinguishable");
}

#[tokio::test]
async fn schema_contains_oauth_tables_bound_to_kanidm_subjects() {
    let database = SqliteDatabase::connect("sqlite::memory:")
        .await
        .expect("database");
    for table in [
        "mcp_clients",
        "mcp_authorization_codes",
        "mcp_access_tokens",
        "mcp_refresh_tokens",
    ] {
        let exists = sqlx::query_scalar::<_, i64>(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?",
        )
        .bind(table)
        .fetch_optional(&database.pool)
        .await
        .expect("schema query")
        .is_some();
        assert!(exists, "{table} must exist");
    }
    let client = McpOAuthClient {
        client_id: "https://client.example.test/mcp.json".into(),
        display_name: "Test client".into(),
        redirect_uris: vec!["https://client.example.test/callback".into()],
    };
    database
        .upsert_mcp_client(&client, UnixMillis::new(0))
        .await
        .expect("client");
    assert_eq!(
        database
            .mcp_client(&client.client_id)
            .await
            .expect("lookup"),
        Some(client.clone())
    );
    let grant = McpAuthorizationGrant {
        actor: actor("https://id.example.test", "alice"),
        client_id: client.client_id.clone(),
        redirect_uri: client.redirect_uris[0].clone(),
        resource_uri: "https://notes.example.test/mcp".into(),
        scopes: vec!["notes:read".into(), "notes:write".into()],
    };
    database
        .issue_mcp_authorization_code(
            "code",
            &registered_client(&client, McpClientRegistrationMethod::Dynamic),
            &grant,
            "challenge",
            UnixMillis::new(100),
            UnixMillis::new(0),
        )
        .await
        .expect("code");
    assert!(
        database
            .exchange_mcp_authorization_code(
                McpAuthorizationCodeExchange {
                    code: "code".into(),
                    client_id: grant.client_id.clone(),
                    redirect_uri: Some(grant.redirect_uri.clone()),
                    resource_uri: grant.resource_uri.clone(),
                    code_challenge: "wrong-challenge".into(),
                    access_token: "wrong-access".into(),
                    refresh_token: "wrong-refresh".into(),
                    access_expires_at: UnixMillis::new(100),
                    refresh_expires_at: UnixMillis::new(1_000),
                },
                UnixMillis::new(1),
            )
            .await
            .expect("wrong PKCE challenge")
            .is_none()
    );
    assert!(
        database
            .exchange_mcp_authorization_code(
                McpAuthorizationCodeExchange {
                    code: "code".into(),
                    client_id: grant.client_id.clone(),
                    redirect_uri: Some(grant.redirect_uri.clone()),
                    resource_uri: grant.resource_uri.clone(),
                    code_challenge: "challenge".into(),
                    access_token: "access".into(),
                    refresh_token: "refresh".into(),
                    access_expires_at: UnixMillis::new(100),
                    refresh_expires_at: UnixMillis::new(1_000),
                },
                UnixMillis::new(1),
            )
            .await
            .expect("exchange")
            .is_some()
    );
    assert!(
        database
            .authenticate_mcp_access_token("access", &grant.resource_uri, UnixMillis::new(2))
            .await
            .expect("access token")
            .is_some()
    );
    assert!(matches!(
        database
            .rotate_mcp_refresh_token(
                McpRefreshTokenRotation {
                    refresh_token: "refresh".into(),
                    client_id: grant.client_id.clone(),
                    resource_uri: grant.resource_uri.clone(),
                    requested_scopes: Some(vec!["notes:delete".into()]),
                    new_access_token: "escalated-access".into(),
                    new_refresh_token: "escalated-refresh".into(),
                    access_expires_at: UnixMillis::new(200),
                    refresh_expires_at: UnixMillis::new(2_000),
                },
                UnixMillis::new(2)
            )
            .await
            .expect("scope escalation"),
        McpRefreshTokenRotationOutcome::InvalidScope
    ));
    assert!(matches!(
        database
            .rotate_mcp_refresh_token(
                McpRefreshTokenRotation {
                    refresh_token: "refresh".into(),
                    client_id: grant.client_id.clone(),
                    resource_uri: grant.resource_uri.clone(),
                    requested_scopes: Some(vec!["notes:read".into()]),
                    new_access_token: "next-access".into(),
                    new_refresh_token: "next-refresh".into(),
                    access_expires_at: UnixMillis::new(200),
                    refresh_expires_at: UnixMillis::new(2_000),
                },
                UnixMillis::new(3)
            )
            .await
            .expect("rotation"),
        McpRefreshTokenRotationOutcome::Rotated { .. }
    ));
    let rotated_actor = database
        .authenticate_mcp_access_token("next-access", &grant.resource_uri, UnixMillis::new(4))
        .await
        .expect("rotated access")
        .expect("authenticated actor");
    assert_eq!(rotated_actor.scopes, vec!["notes:read"]);
    assert!(
        database
            .register_mcp_client_bounded(
                &McpOAuthClient {
                    client_id: "another-client".into(),
                    display_name: "Another client".into(),
                    redirect_uris: vec!["https://other.example.test/callback".into()],
                },
                UnixMillis::new(5),
                10,
            )
            .await
            .expect("registration")
    );
    assert!(matches!(
        database
            .rotate_mcp_refresh_token(
                McpRefreshTokenRotation {
                    refresh_token: "refresh".into(),
                    client_id: "different-client".into(),
                    resource_uri: grant.resource_uri.clone(),
                    requested_scopes: None,
                    new_access_token: "wrong-binding-access".into(),
                    new_refresh_token: "wrong-binding-refresh".into(),
                    access_expires_at: UnixMillis::new(200),
                    refresh_expires_at: UnixMillis::new(2_000),
                },
                UnixMillis::new(6)
            )
            .await
            .expect("wrong binding"),
        McpRefreshTokenRotationOutcome::InvalidToken
    ));
    assert!(
        database
            .authenticate_mcp_access_token("next-access", &grant.resource_uri, UnixMillis::new(7))
            .await
            .expect("access after wrong binding")
            .is_some()
    );
    assert!(matches!(
        database
            .rotate_mcp_refresh_token(
                McpRefreshTokenRotation {
                    refresh_token: "refresh".into(),
                    client_id: grant.client_id.clone(),
                    resource_uri: grant.resource_uri.clone(),
                    requested_scopes: None,
                    new_access_token: "again-access".into(),
                    new_refresh_token: "again-refresh".into(),
                    access_expires_at: UnixMillis::new(200),
                    refresh_expires_at: UnixMillis::new(2_000),
                },
                UnixMillis::new(8)
            )
            .await
            .expect("refresh token replay"),
        McpRefreshTokenRotationOutcome::InvalidToken
    ));
    assert!(
        database
            .authenticate_mcp_access_token("next-access", &grant.resource_uri, UnixMillis::new(9))
            .await
            .expect("access after replay")
            .is_none()
    );
    assert!(matches!(
        database
            .rotate_mcp_refresh_token(
                McpRefreshTokenRotation {
                    refresh_token: "next-refresh".into(),
                    client_id: grant.client_id.clone(),
                    resource_uri: grant.resource_uri.clone(),
                    requested_scopes: None,
                    new_access_token: "post-replay-access".into(),
                    new_refresh_token: "post-replay-refresh".into(),
                    access_expires_at: UnixMillis::new(200),
                    refresh_expires_at: UnixMillis::new(2_000),
                },
                UnixMillis::new(10)
            )
            .await
            .expect("family after replay"),
        McpRefreshTokenRotationOutcome::InvalidToken
    ));
}

#[tokio::test]
async fn explicit_auth_cleanup_prunes_stale_unreferenced_clients() {
    let database = SqliteDatabase::connect("sqlite::memory:")
        .await
        .expect("database");
    database
        .upsert_mcp_client(
            &McpOAuthClient {
                client_id: "stale-client".into(),
                display_name: "Stale client".into(),
                redirect_uris: vec!["https://client.example.test/callback".into()],
            },
            UnixMillis::new(0),
        )
        .await
        .expect("client");
    let now = UnixMillis::new(2 * 24 * 60 * 60 * 1_000);
    let counts = database
        .purge_expired_auth_state(now, UnixMillis::new(24 * 60 * 60 * 1_000))
        .await
        .expect("cleanup");
    assert_eq!(counts.mcp_clients, 1);
    assert!(
        database
            .register_mcp_client_bounded(
                &McpOAuthClient {
                    client_id: "fresh-client".into(),
                    display_name: "Fresh client".into(),
                    redirect_uris: vec!["https://client.example.test/callback".into()],
                },
                now,
                1,
            )
            .await
            .expect("register")
    );
    assert!(
        !database
            .register_mcp_client_bounded(
                &McpOAuthClient {
                    client_id: "overflow-client".into(),
                    display_name: "Overflow client".into(),
                    redirect_uris: vec!["https://client.example.test/callback".into()],
                },
                now,
                1,
            )
            .await
            .expect("register at the bound"),
        "上限に達した場合は登録せずに知らせる"
    );
    assert!(
        database
            .mcp_client("stale-client")
            .await
            .expect("lookup")
            .is_none()
    );
    assert!(
        database
            .mcp_client("overflow-client")
            .await
            .expect("lookup")
            .is_none()
    );
    assert!(
        database
            .mcp_client("fresh-client")
            .await
            .expect("lookup")
            .is_some()
    );
}

#[tokio::test]
async fn metadata_document_clients_do_not_consume_the_dynamic_registration_bound() {
    let database = SqliteDatabase::connect("sqlite::memory:")
        .await
        .expect("database");
    let metadata_client = McpOAuthClient {
        client_id: "https://client.example.test/metadata.json".into(),
        display_name: "Metadata client".into(),
        redirect_uris: vec!["https://client.example.test/callback".into()],
    };
    let grant = McpAuthorizationGrant {
        actor: actor("https://id.example.test", "alice"),
        client_id: metadata_client.client_id.clone(),
        redirect_uri: metadata_client.redirect_uris[0].clone(),
        resource_uri: "https://notes.example.test/mcp".into(),
        scopes: vec!["notes:read".into()],
    };
    database
        .issue_mcp_authorization_code(
            "metadata-code",
            &registered_client(
                &metadata_client,
                McpClientRegistrationMethod::MetadataDocument,
            ),
            &grant,
            "challenge",
            UnixMillis::new(1_000),
            UnixMillis::new(0),
        )
        .await
        .expect("metadata client");

    assert!(
        database
            .register_mcp_client_bounded(
                &McpOAuthClient {
                    client_id: "dynamic-client".into(),
                    display_name: "Dynamic client".into(),
                    redirect_uris: vec!["https://dynamic.example.test/callback".into()],
                },
                UnixMillis::new(1),
                1,
            )
            .await
            .expect("dynamic registration"),
        "metadata document clientはDCRの上限へ含めない"
    );
}

#[tokio::test]
async fn authorization_code_replay_revokes_the_issued_token_family() {
    let database = SqliteDatabase::connect("sqlite::memory:")
        .await
        .expect("database");
    let client = McpOAuthClient {
        client_id: "client".into(),
        display_name: "Client".into(),
        redirect_uris: vec!["https://client.example/callback".into()],
    };
    database
        .upsert_mcp_client(&client, UnixMillis::new(0))
        .await
        .expect("client");
    let grant = McpAuthorizationGrant {
        actor: actor("https://id.example", "alice"),
        client_id: client.client_id.clone(),
        redirect_uri: client.redirect_uris[0].clone(),
        resource_uri: "https://notes.example/mcp".into(),
        scopes: vec!["notes:read".into()],
    };
    database
        .issue_mcp_authorization_code(
            "code",
            &registered_client(&client, McpClientRegistrationMethod::Dynamic),
            &grant,
            "challenge",
            UnixMillis::new(100),
            UnixMillis::new(0),
        )
        .await
        .expect("authorization code");
    let exchanged = database
        .exchange_mcp_authorization_code(
            McpAuthorizationCodeExchange {
                code: "code".into(),
                client_id: grant.client_id.clone(),
                redirect_uri: Some(grant.redirect_uri.clone()),
                resource_uri: grant.resource_uri.clone(),
                code_challenge: "challenge".into(),
                access_token: "access".into(),
                refresh_token: "refresh".into(),
                access_expires_at: UnixMillis::new(500),
                refresh_expires_at: UnixMillis::new(900),
            },
            UnixMillis::new(1),
        )
        .await
        .expect("first exchange");
    assert!(exchanged.is_some());
    assert!(
        database
            .authenticate_mcp_access_token("access", &grant.resource_uri, UnixMillis::new(2))
            .await
            .expect("access token")
            .is_some()
    );
    assert!(
        database
            .register_mcp_client_bounded(
                &McpOAuthClient {
                    client_id: "cleanup-trigger".into(),
                    display_name: "Cleanup trigger".into(),
                    redirect_uris: vec!["https://other.example/callback".into()],
                },
                UnixMillis::new(200),
                10,
            )
            .await
            .expect("cleanup while token family is active")
    );

    let replay = database
        .exchange_mcp_authorization_code(
            McpAuthorizationCodeExchange {
                code: "code".into(),
                client_id: grant.client_id.clone(),
                redirect_uri: Some(grant.redirect_uri.clone()),
                resource_uri: grant.resource_uri.clone(),
                code_challenge: "challenge".into(),
                access_token: "attacker-access".into(),
                refresh_token: "attacker-refresh".into(),
                access_expires_at: UnixMillis::new(500),
                refresh_expires_at: UnixMillis::new(900),
            },
            UnixMillis::new(201),
        )
        .await
        .expect("replayed exchange");
    assert!(replay.is_none());
    assert!(
        database
            .authenticate_mcp_access_token("access", &grant.resource_uri, UnixMillis::new(202))
            .await
            .expect("revoked access token")
            .is_none()
    );
    assert!(matches!(
        database
            .rotate_mcp_refresh_token(
                McpRefreshTokenRotation {
                    refresh_token: "refresh".into(),
                    client_id: grant.client_id,
                    resource_uri: grant.resource_uri,
                    requested_scopes: None,
                    new_access_token: "next-access".into(),
                    new_refresh_token: "next-refresh".into(),
                    access_expires_at: UnixMillis::new(500),
                    refresh_expires_at: UnixMillis::new(900),
                },
                UnixMillis::new(203),
            )
            .await
            .expect("revoked refresh token"),
        McpRefreshTokenRotationOutcome::InvalidToken
    ));
}

#[tokio::test]
async fn token_issuance_failure_rolls_back_authorization_code_consumption() {
    let database = SqliteDatabase::connect("sqlite::memory:")
        .await
        .expect("database");
    let client = McpOAuthClient {
        client_id: "client".into(),
        display_name: "Client".into(),
        redirect_uris: vec!["https://client.example/callback".into()],
    };
    database
        .upsert_mcp_client(&client, UnixMillis::new(0))
        .await
        .expect("client");
    let grant = McpAuthorizationGrant {
        actor: actor("https://id.example", "alice"),
        client_id: client.client_id.clone(),
        redirect_uri: client.redirect_uris[0].clone(),
        resource_uri: "https://notes.example/mcp".into(),
        scopes: vec!["notes:read".into()],
    };
    for code in ["first-code", "retryable-code"] {
        database
            .issue_mcp_authorization_code(
                code,
                &registered_client(&client, McpClientRegistrationMethod::Dynamic),
                &grant,
                "challenge",
                UnixMillis::new(1_000),
                UnixMillis::new(0),
            )
            .await
            .expect("authorization code");
    }
    database
        .exchange_mcp_authorization_code(
            McpAuthorizationCodeExchange {
                code: "first-code".into(),
                client_id: grant.client_id.clone(),
                redirect_uri: Some(grant.redirect_uri.clone()),
                resource_uri: grant.resource_uri.clone(),
                code_challenge: "challenge".into(),
                access_token: "colliding-access".into(),
                refresh_token: "first-refresh".into(),
                access_expires_at: UnixMillis::new(500),
                refresh_expires_at: UnixMillis::new(900),
            },
            UnixMillis::new(1),
        )
        .await
        .expect("first exchange")
        .expect("first grant");

    let failed = database
        .exchange_mcp_authorization_code(
            McpAuthorizationCodeExchange {
                code: "retryable-code".into(),
                client_id: grant.client_id.clone(),
                redirect_uri: Some(grant.redirect_uri.clone()),
                resource_uri: grant.resource_uri.clone(),
                code_challenge: "challenge".into(),
                access_token: "colliding-access".into(),
                refresh_token: "failed-refresh".into(),
                access_expires_at: UnixMillis::new(500),
                refresh_expires_at: UnixMillis::new(900),
            },
            UnixMillis::new(2),
        )
        .await;
    assert!(matches!(failed, Err(SqliteStoreError::Database(_))));

    let retried = database
        .exchange_mcp_authorization_code(
            McpAuthorizationCodeExchange {
                code: "retryable-code".into(),
                client_id: grant.client_id,
                redirect_uri: Some(grant.redirect_uri.clone()),
                resource_uri: grant.resource_uri,
                code_challenge: "challenge".into(),
                access_token: "retry-access".into(),
                refresh_token: "retry-refresh".into(),
                access_expires_at: UnixMillis::new(500),
                refresh_expires_at: UnixMillis::new(900),
            },
            UnixMillis::new(3),
        )
        .await
        .expect("retry after rollback");
    assert!(retried.is_some());
}
