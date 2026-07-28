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
        .issue_mcp_authorization_code("code", &grant, "challenge", UnixMillis::new(100))
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
        database
            .mcp_client("stale-client")
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
        .issue_mcp_authorization_code("code", &grant, "challenge", UnixMillis::new(100))
        .await
        .expect("authorization code");
    let exchanged = database
        .exchange_mcp_authorization_code(
            McpAuthorizationCodeExchange {
                code: "code".into(),
                client_id: grant.client_id.clone(),
                redirect_uri: None,
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
                redirect_uri: None,
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
            .issue_mcp_authorization_code(code, &grant, "challenge", UnixMillis::new(1_000))
            .await
            .expect("authorization code");
    }
    database
        .exchange_mcp_authorization_code(
            McpAuthorizationCodeExchange {
                code: "first-code".into(),
                client_id: grant.client_id.clone(),
                redirect_uri: None,
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
                redirect_uri: None,
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
                redirect_uri: None,
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
