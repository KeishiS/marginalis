use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use marginalis_application::{
    Clock, McpAuthenticatedPrincipal, McpAuthorizationRequest, McpClientMetadataResolver,
    McpClientMetadataResolverError, McpOAuthApplication, McpRegisteredOAuthClient,
    McpResourcePolicy, McpScopeCeilingRepository, McpScopeCeilingSetting,
    McpScopeCeilingUseCaseError, McpStoredClientAuthorization, McpStoredScopeCeilings,
    McpTimestamp as UnixMillis, Random, StorageError,
};
use marginalis_domain::EntityId as ApplicationEntityId;
use sha2::{Digest, Sha256};

use super::*;

/// 試験で使うMCP resourceのURI。
const RESOURCE_URI: &str = "https://notes.example.test/mcp";
/// 試験で使うclientのredirect先。
const REDIRECT_URI: &str = "https://client.example.test/callback";

/// 決定的な時刻とtokenを供給する試験用の実装。
struct FixedClock(i64);

impl Clock for FixedClock {
    fn now(&self) -> marginalis_domain::UnixMillis {
        marginalis_domain::UnixMillis::new(self.0)
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
    ) -> Result<Option<McpOAuthClient>, McpClientMetadataResolverError> {
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

/// 既定のredirect先を1つ持つclient。
fn oauth_client(client_id: &str, display_name: &str) -> McpOAuthClient {
    McpOAuthClient {
        client_id: client_id.into(),
        display_name: display_name.into(),
        redirect_uris: vec![REDIRECT_URI.into()],
    }
}

/// aliceが既定resourceへ同意した内容。scope以外を変えたい試験はstruct更新記法で上書きする。
fn grant(client: &McpOAuthClient, scopes: &[&str]) -> McpAuthorizationGrant {
    McpAuthorizationGrant {
        principal: principal(ISSUER, "alice"),
        client_id: client.client_id.clone(),
        redirect_uri: McpResolvedRedirectUri::Supplied(client.redirect_uris[0].clone()),
        resource_uri: RESOURCE_URI.into(),
        scopes: scopes.iter().map(|scope| (*scope).to_owned()).collect(),
    }
}

/// verifierに対するS256のcode_challengeを計算する。
fn pkce_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

/// challenge固定値`challenge`で認可codeを発行する。
async fn issue_code(
    database: &SqliteDatabase,
    code: &str,
    client: &McpOAuthClient,
    registration_method: McpClientRegistrationMethod,
    grant: &McpAuthorizationGrant,
    expires_at: i64,
    now: i64,
) {
    database
        .issue_mcp_authorization_code(
            code,
            &registered_client(client, registration_method),
            grant,
            "challenge",
            UnixMillis::new(expires_at),
            UnixMillis::new(now),
        )
        .await
        .expect("authorization code");
}

/// 同意内容と一致するcode交換要求。差分がある試験はstruct更新記法で上書きする。
fn code_exchange(
    code: &str,
    grant: &McpAuthorizationGrant,
    access_token: &str,
    refresh_token: &str,
) -> McpAuthorizationCodeExchange {
    McpAuthorizationCodeExchange {
        code: code.into(),
        client_id: grant.client_id.clone(),
        redirect_uri: Some(grant.redirect_uri.as_str().to_owned()),
        resource_uri: grant.resource_uri.clone(),
        code_challenge: "challenge".into(),
        access_token: access_token.into(),
        refresh_token: refresh_token.into(),
        access_expires_at: UnixMillis::new(500),
        refresh_expires_at: UnixMillis::new(900),
    }
}

/// 同じclientとresourceに対するrefresh token回転の要求。
fn refresh_rotation(
    refresh_token: &str,
    grant: &McpAuthorizationGrant,
    new_access_token: &str,
    new_refresh_token: &str,
) -> McpRefreshTokenRotation {
    McpRefreshTokenRotation {
        refresh_token: refresh_token.into(),
        client_id: grant.client_id.clone(),
        resource_uri: grant.resource_uri.clone(),
        requested_scopes: None,
        new_access_token: new_access_token.into(),
        new_refresh_token: new_refresh_token.into(),
        access_expires_at: UnixMillis::new(200),
        refresh_expires_at: UnixMillis::new(2_000),
    }
}

/// code交換を実行し、成立した場合の同意内容を返す。保存層のerrorは試験の失敗として扱う。
async fn exchange(
    database: &SqliteDatabase,
    request: McpAuthorizationCodeExchange,
    now: i64,
) -> Option<McpAuthorizationGrant> {
    database
        .exchange_mcp_authorization_code(request, UnixMillis::new(now))
        .await
        .expect("code exchange")
}

/// access tokenを検証し、認証済みprincipalを返す。
async fn authenticate(
    database: &SqliteDatabase,
    token: &str,
    grant: &McpAuthorizationGrant,
    now: i64,
) -> Option<McpAuthenticatedPrincipal> {
    database
        .authenticate_mcp_access_token(token, &grant.resource_uri, UnixMillis::new(now))
        .await
        .expect("access token authentication")
}

/// refresh tokenの回転を試み、その結果を返す。
async fn rotate(
    database: &SqliteDatabase,
    rotation: McpRefreshTokenRotation,
    now: i64,
) -> McpRefreshTokenRotationOutcome {
    database
        .rotate_mcp_refresh_token(rotation, UnixMillis::new(now))
        .await
        .expect("refresh token rotation")
}

/// domain側の時刻。scope上限repositoryと定期削除はこちらを受け取る。
fn at(value: i64) -> marginalis_domain::UnixMillis {
    marginalis_domain::UnixMillis::new(value)
}

/// principal別のscope上限を生SQLでrevision 1としてseedする。
async fn seed_principal_scope_ceiling(
    database: &SqliteDatabase,
    subject: &str,
    scopes: &str,
    updated_at: i64,
) {
    sqlx::query(
        "INSERT INTO mcp_principal_scope_ceilings
             (principal_id, scopes, revision, updated_at_ms)
         VALUES (?, ?, 1, ?)",
    )
    .bind(user(subject).principal_id().get())
    .bind(scopes)
    .bind(updated_at)
    .execute(&database.pool)
    .await
    .expect("principal scope ceiling");
}

/// client別のscope上限を生SQLでrevision 1としてseedする。
async fn seed_client_scope_ceiling(
    database: &SqliteDatabase,
    subject: &str,
    client_id: &str,
    scopes: &str,
    updated_at: i64,
) {
    sqlx::query(
        "INSERT INTO mcp_client_scope_ceilings
             (principal_id, client_id, scopes, revision, updated_at_ms)
         VALUES (?, ?, ?, 1, ?)",
    )
    .bind(user(subject).principal_id().get())
    .bind(client_id)
    .bind(scopes)
    .bind(updated_at)
    .execute(&database.pool)
    .await
    .expect("client scope ceiling");
}

/// 利用者から見えるclient同意の一覧を読み出す。
async fn client_authorizations(
    database: &SqliteDatabase,
    owner: &Actor,
    now: i64,
) -> Vec<McpStoredClientAuthorization> {
    McpScopeCeilingRepository::client_authorizations(database, owner, at(now))
        .await
        .expect("client authorizations")
}

/// 既定のresource policy。notes系3 scopeへ対応し、既定scopeは`notes:read`とする。
fn resource_policy() -> McpResourcePolicy {
    McpResourcePolicy::new(
        RESOURCE_URI.into(),
        "Test resource".into(),
        vec![
            "notes:read".into(),
            "notes:write".into(),
            "notes:delete".into(),
        ],
        vec!["notes:read".into()],
    )
    .expect("valid test resource policy")
}

/// application層を決定的な時刻と乱数でSQLiteの実装へ結線する。
fn oauth_application(database: &SqliteDatabase) -> McpOAuthApplication {
    McpOAuthApplication::new(
        Arc::new(database.clone()),
        Arc::new(database.clone()),
        Arc::new(database.clone()),
        Arc::new(FixedClock(1_000)),
        Arc::new(SequentialRandom(std::sync::Mutex::new(0))),
        resource_policy(),
    )
}

/// 事前登録のないClient ID Metadata Document clientでも、認可からMCP利用まで通ることを確認する。
///
/// `mcp_authorization_codes.client_id`は`mcp_clients`への外部keyを持つため、解決しただけで
/// 登録していないclientでは認可code発行が失敗していた。applicationとSQLiteを実際に結線し、
/// 取得だけを差し替えて経路全体を確認する。
#[tokio::test]
async fn client_id_metadata_document_clients_complete_the_authorization_flow() {
    let database = database().await;
    let client = oauth_client(
        "https://client.example.test/oauth/metadata.json",
        "Metadata document client",
    );
    let resolver = Arc::new(StaticMetadataResolver {
        client: client.clone(),
        calls: AtomicUsize::new(0),
    });
    let application = oauth_application(&database).with_client_metadata_resolver(resolver.clone());

    let verifier = "a".repeat(43);
    let validated = application
        .validate_authorization_request(&McpAuthorizationRequest {
            client_id: client.client_id.clone(),
            redirect_uri: None,
            resource_uri: RESOURCE_URI.into(),
            scopes: vec!["notes:read".into(), "notes:write".into()],
            code_challenge: pkce_challenge(&verifier),
        })
        .await
        .expect("validated authorization request");
    assert_eq!(validated.client, client);
    assert!(!validated.redirect_uri.was_supplied());
    assert_eq!(
        validated.registration_method,
        McpClientRegistrationMethod::MetadataDocument
    );
    seed_principal_scope_ceiling(&database, "alice", "notes:read", 1_000).await;

    let code = application
        .authorize(user("alice"), validated)
        .await
        .expect("authorization code");
    let authorizations = application
        .client_authorizations(user("alice"))
        .await
        .expect("client authorizations");
    assert_eq!(authorizations.len(), 1);
    assert!(!authorizations[0].scope_ceiling.configured);
    assert_eq!(
        authorizations[0].scope_ceiling.setting,
        McpScopeCeilingSetting {
            scopes: vec![
                "notes:read".into(),
                "notes:write".into(),
                "notes:delete".into(),
            ],
            revision: 0,
        },
        "applicationが未設定のクライアント上限を対応scope全体へ解決する"
    );
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
            redirect_uri: Some(client.redirect_uris[0].clone()),
            resource_uri: RESOURCE_URI.into(),
            scopes: vec!["notes:read".into()],
            code_challenge: pkce_challenge(&"b".repeat(43)),
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
            None,
            RESOURCE_URI.to_owned(),
            verifier,
        )
        .await
        .expect("token pair");
    assert_eq!(pair.scope, "notes:read");

    let authenticated = application
        .authenticate(&pair.access_token, RESOURCE_URI)
        .await
        .expect("authentication")
        .expect("authenticated actor");
    assert_eq!(
        authenticated.actor.authenticated_identity().subject(),
        "alice"
    );
    assert_eq!(authenticated.scopes, vec!["notes:read"]);
    assert_eq!(
        application
            .replace_principal_scope_ceiling(user("alice"), vec!["unknown:scope".into()], 1)
            .await,
        Err(McpScopeCeilingUseCaseError::Invalid),
        "未対応scopeを保存層へ渡さない"
    );
}

#[tokio::test]
async fn scope_ceilings_are_bound_to_the_principal_and_client() {
    let database = database().await;
    let client = oauth_client("scope-limited-client", "Scope limited client");
    assert!(
        database
            .register_mcp_client_bounded(&client, UnixMillis::new(1), 10)
            .await
            .expect("client registration")
    );
    seed_principal_scope_ceiling(&database, "alice", "notes:write notes:read", 2).await;
    seed_client_scope_ceiling(&database, "alice", &client.client_id, "notes:read", 3).await;

    assert_eq!(
        database
            .scope_ceilings(&user("alice"), &client.client_id)
            .await
            .expect("scope ceilings"),
        McpStoredScopeCeilings {
            principal: Some(McpScopeCeilingSetting {
                scopes: vec!["notes:write".into(), "notes:read".into()],
                revision: 1,
            }),
            client: Some(McpScopeCeilingSetting {
                scopes: vec!["notes:read".into()],
                revision: 1,
            }),
        }
    );
    assert_eq!(
        database
            .scope_ceilings(&user("bob"), &client.client_id)
            .await
            .expect("other principal scope ceilings"),
        McpStoredScopeCeilings::default(),
        "別の利用者の設定は共有しない"
    );
}

#[tokio::test]
async fn aliases_share_mcp_authorization_ceilings_and_revocation() {
    let database = database().await;
    let primary = user("alice");
    let alias = add_alias(
        &database,
        &primary,
        "https://replacement-id.example.test",
        "alice-after-migration",
    )
    .await;
    let client = oauth_client("identity-migration-client", "Identity migration client");
    database
        .upsert_mcp_client(&client, UnixMillis::new(0))
        .await
        .expect("client");

    let alias_grant = McpAuthorizationGrant {
        principal: principal(
            alias.authenticated_identity().issuer(),
            alias.authenticated_identity().subject(),
        ),
        client_id: client.client_id.clone(),
        redirect_uri: McpResolvedRedirectUri::Supplied(client.redirect_uris[0].clone()),
        resource_uri: RESOURCE_URI.into(),
        scopes: vec!["notes:read".into(), "notes:write".into()],
    };
    issue_code(
        &database,
        "alias-code",
        &client,
        McpClientRegistrationMethod::Dynamic,
        &alias_grant,
        2_000,
        0,
    )
    .await;
    let mut alias_exchange =
        code_exchange("alias-code", &alias_grant, "alias-access", "alias-refresh");
    alias_exchange.access_expires_at = UnixMillis::new(2_000);
    alias_exchange.refresh_expires_at = UnixMillis::new(3_000);
    exchange(&database, alias_exchange, 1)
        .await
        .expect("alias grant");

    let stored = authenticate(&database, "alias-access", &alias_grant, 2)
        .await
        .expect("stored alias principal");
    assert_eq!(
        stored.principal.issuer(),
        alias.authenticated_identity().issuer()
    );
    assert_eq!(
        stored.principal.subject(),
        alias.authenticated_identity().subject()
    );
    let authenticated = oauth_application(&database)
        .authenticate("alias-access", RESOURCE_URI)
        .await
        .expect("application authentication")
        .expect("active alias token");
    assert_eq!(authenticated.actor.principal_id(), primary.principal_id());
    assert_eq!(
        authenticated.actor.authenticated_identity(),
        alias.authenticated_identity()
    );

    let bob_grant = McpAuthorizationGrant {
        principal: principal(ISSUER, "bob"),
        client_id: client.client_id.clone(),
        redirect_uri: McpResolvedRedirectUri::Supplied(client.redirect_uris[0].clone()),
        resource_uri: RESOURCE_URI.into(),
        scopes: vec!["notes:read".into()],
    };
    issue_code(
        &database,
        "bob-code",
        &client,
        McpClientRegistrationMethod::Dynamic,
        &bob_grant,
        2_000,
        0,
    )
    .await;
    let mut bob_exchange = code_exchange("bob-code", &bob_grant, "bob-access", "bob-refresh");
    bob_exchange.access_expires_at = UnixMillis::new(2_000);
    bob_exchange.refresh_expires_at = UnixMillis::new(3_000);
    exchange(&database, bob_exchange, 1)
        .await
        .expect("bob grant");

    database
        .replace_principal_scope_ceiling(&primary, &["notes:read".into()], 0, at(3))
        .await
        .expect("restrict primary principal");
    assert!(
        authenticate(&database, "alias-access", &alias_grant, 4)
            .await
            .is_none(),
        "primary identityからの上限変更はalias発行tokenにも適用する"
    );
    assert!(
        authenticate(&database, "bob-access", &bob_grant, 4)
            .await
            .is_some(),
        "別principalのtokenは失効しない"
    );

    let alias_read_grant = McpAuthorizationGrant {
        scopes: vec!["notes:read".into()],
        ..alias_grant
    };
    issue_code(
        &database,
        "alias-read-code",
        &client,
        McpClientRegistrationMethod::Dynamic,
        &alias_read_grant,
        2_000,
        5,
    )
    .await;
    let mut alias_read_exchange = code_exchange(
        "alias-read-code",
        &alias_read_grant,
        "alias-read-access",
        "alias-read-refresh",
    );
    alias_read_exchange.access_expires_at = UnixMillis::new(2_000);
    alias_read_exchange.refresh_expires_at = UnixMillis::new(3_000);
    exchange(&database, alias_read_exchange, 6)
        .await
        .expect("alias read grant");
    database
        .revoke_mcp_client_tokens(
            primary.authenticated_identity().issuer(),
            primary.authenticated_identity().subject(),
            &client.client_id,
            UnixMillis::new(7),
        )
        .await
        .expect("revoke through primary identity");
    assert!(
        authenticate(&database, "alias-read-access", &alias_read_grant, 8)
            .await
            .is_none(),
        "primary identityからの失効はalias発行tokenにも適用する"
    );
    assert!(
        authenticate(&database, "bob-access", &bob_grant, 8)
            .await
            .is_some()
    );
}

#[tokio::test]
async fn client_authorizations_are_owner_scoped_and_record_use_and_revocation() {
    let database = database().await;
    let client = oauth_client("managed-client", "Managed client");
    let alice = user("alice");
    let grant = grant(&client, &["notes:read", "notes:write"]);
    issue_code(
        &database,
        "managed-code",
        &client,
        McpClientRegistrationMethod::MetadataDocument,
        &grant,
        100,
        10,
    )
    .await;

    assert!(
        client_authorizations(&database, &user("bob"), 11)
            .await
            .is_empty()
    );
    let authorizations = client_authorizations(&database, &alice, 11).await;
    assert_eq!(authorizations.len(), 1);
    assert_eq!(authorizations[0].client_id, client.client_id);
    assert_eq!(authorizations[0].authorized_at.get(), 10);
    assert_eq!(authorizations[0].last_used_at, None);
    assert_eq!(
        authorizations[0].scope_ceiling, None,
        "同意履歴を未設定の上限として扱わない"
    );
    assert!(authorizations[0].active);

    exchange(
        &database,
        code_exchange("managed-code", &grant, "managed-access", "managed-refresh"),
        12,
    )
    .await
    .expect("grant");
    authenticate(&database, "managed-access", &grant, 13)
        .await
        .expect("principal");
    let used = client_authorizations(&database, &alice, 14).await;
    assert_eq!(used[0].last_used_at.map(|value| value.get()), Some(13));

    // 上限は今後の認可を制限する設定であり、それ自体は権限を付与しない。まだ同意していないscopeも
    // 上限へ含められる。ここで制限できないと、狭めた上限を広げられず復旧できなくなる。
    database
        .replace_client_scope_ceiling(
            &alice,
            &client.client_id,
            &[
                "notes:read".into(),
                "notes:write".into(),
                "notes:delete".into(),
            ],
            0,
            at(15),
        )
        .await
        .expect("client ceiling beyond the granted scopes");
    let widened = client_authorizations(&database, &alice, 15).await;
    assert!(
        widened[0].active,
        "同意済みscopeを含む上限は既存tokenを失効させない"
    );

    database
        .replace_client_scope_ceiling(&alice, &client.client_id, &["notes:read".into()], 1, at(16))
        .await
        .expect("restricted client ceiling");
    let restricted = client_authorizations(&database, &alice, 17).await;
    assert_eq!(
        restricted[0].scope_ceiling,
        Some(McpScopeCeilingSetting {
            scopes: vec!["notes:read".into()],
            revision: 2,
        })
    );
    assert!(!restricted[0].active, "上限外のtoken familyは失効する");

    // 解除にはrevisionの一致を求め、解除後は未設定へ戻す。
    assert!(matches!(
        database
            .delete_client_scope_ceiling(&alice, &client.client_id, 1, at(17))
            .await,
        Err(StorageError::Conflict)
    ));
    database
        .delete_client_scope_ceiling(&alice, &client.client_id, 2, at(17))
        .await
        .expect("cleared client ceiling");
    let cleared = client_authorizations(&database, &alice, 17).await;
    assert_eq!(
        cleared[0].scope_ceiling, None,
        "解除した上限は未設定として扱う"
    );
    // 解除は上限を広げる操作なので、失効済みのtoken familyを復活させない。
    assert!(!cleared[0].active);

    database
        .revoke_mcp_client_tokens(
            alice.authenticated_identity().issuer(),
            alice.authenticated_identity().subject(),
            &client.client_id,
            UnixMillis::new(18),
        )
        .await
        .expect("revocation");
    let revoked = client_authorizations(&database, &alice, 19).await;
    assert!(!revoked[0].active);
    assert!(
        authenticate(&database, "managed-access", &grant, 19)
            .await
            .is_none()
    );
}

#[tokio::test]
async fn rfc7009_revocation_revokes_the_whole_token_family() {
    let database = database().await;
    let client = oauth_client("public-client", "Public client");
    database
        .upsert_mcp_client(&client, UnixMillis::new(0))
        .await
        .expect("client");
    let grant = grant(&client, &["notes:read"]);
    issue_code(
        &database,
        "code",
        &client,
        McpClientRegistrationMethod::Dynamic,
        &grant,
        100,
        0,
    )
    .await;
    exchange(
        &database,
        code_exchange("code", &grant, "access", "refresh"),
        1,
    )
    .await
    .expect("grant");

    database
        .revoke_mcp_token("refresh", &client.client_id, UnixMillis::new(2))
        .await
        .expect("revocation");
    assert!(authenticate(&database, "access", &grant, 3).await.is_none());
    database
        .revoke_mcp_token("unknown", &client.client_id, UnixMillis::new(4))
        .await
        .expect("unknown token is indistinguishable");
}

#[tokio::test]
async fn replacing_scope_ceilings_is_revision_guarded_and_revokes_existing_grants() {
    let database = database().await;
    let client = oauth_client("scope-settings-client", "Scope settings client");
    database
        .upsert_mcp_client(&client, UnixMillis::new(0))
        .await
        .expect("client");
    let actor = user("alice");
    let grant = grant(&client, &["notes:read", "notes:write"]);
    for code in ["token-code", "pending-code"] {
        issue_code(
            &database,
            code,
            &client,
            McpClientRegistrationMethod::Dynamic,
            &grant,
            100,
            0,
        )
        .await;
    }
    exchange(
        &database,
        code_exchange("token-code", &grant, "access", "refresh"),
        1,
    )
    .await
    .expect("grant");

    let setting = database
        .replace_principal_scope_ceiling(&actor, &["notes:read".into()], 0, at(2))
        .await
        .expect("principal scope ceiling");
    assert_eq!(
        setting,
        McpScopeCeilingSetting {
            scopes: vec!["notes:read".into()],
            revision: 1,
        }
    );
    assert!(
        authenticate(&database, "access", &grant, 3).await.is_none(),
        "上限変更前のtokenを直ちに失効する"
    );
    assert!(
        exchange(
            &database,
            code_exchange("pending-code", &grant, "late-access", "late-refresh"),
            3,
        )
        .await
        .is_none(),
        "上限変更前の認可codeも失効する"
    );

    let read_grant = McpAuthorizationGrant {
        scopes: vec!["notes:read".into()],
        ..grant.clone()
    };
    for code in ["preserved-token-code", "preserved-pending-code"] {
        issue_code(
            &database,
            code,
            &client,
            McpClientRegistrationMethod::Dynamic,
            &read_grant,
            100,
            3,
        )
        .await;
    }
    exchange(
        &database,
        code_exchange(
            "preserved-token-code",
            &read_grant,
            "preserved-access",
            "preserved-refresh",
        ),
        3,
    )
    .await
    .expect("read-only grant");
    assert_eq!(
        database
            .replace_principal_scope_ceiling(
                &actor,
                &["notes:read".into(), "notes:write".into()],
                1,
                at(4),
            )
            .await
            .expect("updated principal scope ceiling")
            .revision,
        2
    );
    assert!(
        authenticate(&database, "preserved-access", &read_grant, 5)
            .await
            .is_some(),
        "上限内のtokenは上限を広げても失効しない"
    );
    assert!(
        exchange(
            &database,
            code_exchange(
                "preserved-pending-code",
                &read_grant,
                "preserved-late-access",
                "preserved-late-refresh",
            ),
            5,
        )
        .await
        .is_some(),
        "上限内の認可codeは上限を広げても失効しない"
    );
    assert!(matches!(
        database
            .replace_principal_scope_ceiling(&actor, &["notes:read".into()], 1, at(5))
            .await,
        Err(StorageError::Conflict)
    ));

    assert_eq!(
        database
            .replace_client_scope_ceiling(
                &actor,
                &client.client_id,
                &["notes:read".into()],
                0,
                at(6),
            )
            .await
            .expect("client scope ceiling")
            .revision,
        1
    );
    assert!(matches!(
        database
            .replace_client_scope_ceiling(
                &actor,
                "unknown-client",
                &["notes:read".into()],
                0,
                at(7)
            )
            .await,
        Err(StorageError::NotFound)
    ));
}

#[tokio::test]
async fn schema_contains_oauth_tables_bound_to_principals() {
    let database = database().await;
    for table in [
        "mcp_clients",
        "mcp_principal_scope_ceilings",
        "mcp_client_scope_ceilings",
        "mcp_client_authorizations",
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
    let client = oauth_client("https://client.example.test/mcp.json", "Test client");
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
    let grant = grant(&client, &["notes:read", "notes:write"]);
    issue_code(
        &database,
        "code",
        &client,
        McpClientRegistrationMethod::Dynamic,
        &grant,
        100,
        0,
    )
    .await;
    assert!(
        exchange(
            &database,
            McpAuthorizationCodeExchange {
                code_challenge: "wrong-challenge".into(),
                access_expires_at: UnixMillis::new(100),
                refresh_expires_at: UnixMillis::new(1_000),
                ..code_exchange("code", &grant, "wrong-access", "wrong-refresh")
            },
            1,
        )
        .await
        .is_none()
    );
    assert!(
        exchange(
            &database,
            McpAuthorizationCodeExchange {
                redirect_uri: None,
                access_expires_at: UnixMillis::new(100),
                refresh_expires_at: UnixMillis::new(1_000),
                ..code_exchange(
                    "code",
                    &grant,
                    "missing-redirect-access",
                    "missing-redirect-refresh",
                )
            },
            1,
        )
        .await
        .is_none()
    );
    assert!(
        exchange(
            &database,
            McpAuthorizationCodeExchange {
                access_expires_at: UnixMillis::new(100),
                refresh_expires_at: UnixMillis::new(1_000),
                ..code_exchange("code", &grant, "access", "refresh")
            },
            1,
        )
        .await
        .is_some()
    );
    assert!(authenticate(&database, "access", &grant, 2).await.is_some());
    assert!(matches!(
        rotate(
            &database,
            McpRefreshTokenRotation {
                requested_scopes: Some(vec!["notes:delete".into()]),
                ..refresh_rotation("refresh", &grant, "escalated-access", "escalated-refresh")
            },
            2,
        )
        .await,
        McpRefreshTokenRotationOutcome::InvalidScope
    ));
    assert!(matches!(
        rotate(
            &database,
            McpRefreshTokenRotation {
                requested_scopes: Some(vec!["notes:read".into()]),
                ..refresh_rotation("refresh", &grant, "next-access", "next-refresh")
            },
            3,
        )
        .await,
        McpRefreshTokenRotationOutcome::Rotated { .. }
    ));
    let rotated_actor = authenticate(&database, "next-access", &grant, 4)
        .await
        .expect("authenticated actor");
    assert_eq!(rotated_actor.scopes, vec!["notes:read"]);
    assert!(
        database
            .register_mcp_client_bounded(
                &oauth_client("another-client", "Another client"),
                UnixMillis::new(5),
                10,
            )
            .await
            .expect("registration")
    );
    assert!(matches!(
        rotate(
            &database,
            McpRefreshTokenRotation {
                client_id: "different-client".into(),
                ..refresh_rotation(
                    "refresh",
                    &grant,
                    "wrong-binding-access",
                    "wrong-binding-refresh",
                )
            },
            6,
        )
        .await,
        McpRefreshTokenRotationOutcome::InvalidToken
    ));
    assert!(
        authenticate(&database, "next-access", &grant, 7)
            .await
            .is_some()
    );
    assert!(matches!(
        rotate(
            &database,
            refresh_rotation("refresh", &grant, "again-access", "again-refresh"),
            8,
        )
        .await,
        McpRefreshTokenRotationOutcome::InvalidToken
    ));
    assert!(
        authenticate(&database, "next-access", &grant, 9)
            .await
            .is_none()
    );
    assert!(matches!(
        rotate(
            &database,
            refresh_rotation(
                "next-refresh",
                &grant,
                "post-replay-access",
                "post-replay-refresh",
            ),
            10,
        )
        .await,
        McpRefreshTokenRotationOutcome::InvalidToken
    ));
}

#[tokio::test]
async fn explicit_auth_cleanup_prunes_stale_unreferenced_clients() {
    let database = database().await;
    database
        .upsert_mcp_client(
            &oauth_client("stale-client", "Stale client"),
            UnixMillis::new(0),
        )
        .await
        .expect("client");
    database
        .upsert_mcp_client(
            &oauth_client("configured-client", "Configured client"),
            UnixMillis::new(0),
        )
        .await
        .expect("configured client");
    seed_client_scope_ceiling(&database, "alice", "configured-client", "notes:read", 0).await;
    let now_millis = 2 * 24 * 60 * 60 * 1_000;
    let counts = database
        .purge_expired_operational_state(at(now_millis), at(24 * 60 * 60 * 1_000))
        .await
        .expect("cleanup");
    assert_eq!(counts.mcp_clients, 1);
    assert!(
        database
            .register_mcp_client_bounded(
                &oauth_client("fresh-client", "Fresh client"),
                UnixMillis::new(now_millis),
                2,
            )
            .await
            .expect("register")
    );
    assert!(
        !database
            .register_mcp_client_bounded(
                &oauth_client("overflow-client", "Overflow client"),
                UnixMillis::new(now_millis),
                2,
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
    assert!(
        database
            .mcp_client("configured-client")
            .await
            .expect("lookup")
            .is_some(),
        "scope上限があるclientは設定を保つため削除しない"
    );
}

#[tokio::test]
async fn metadata_document_clients_do_not_consume_the_dynamic_registration_bound() {
    let database = database().await;
    let metadata_client = oauth_client(
        "https://client.example.test/metadata.json",
        "Metadata client",
    );
    let grant = grant(&metadata_client, &["notes:read"]);
    issue_code(
        &database,
        "metadata-code",
        &metadata_client,
        McpClientRegistrationMethod::MetadataDocument,
        &grant,
        1_000,
        0,
    )
    .await;

    assert!(
        database
            .register_mcp_client_bounded(
                &oauth_client("dynamic-client", "Dynamic client"),
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
    let database = database().await;
    let client = oauth_client("client", "Client");
    database
        .upsert_mcp_client(&client, UnixMillis::new(0))
        .await
        .expect("client");
    let grant = grant(&client, &["notes:read"]);
    issue_code(
        &database,
        "code",
        &client,
        McpClientRegistrationMethod::Dynamic,
        &grant,
        100,
        0,
    )
    .await;
    let exchanged = exchange(
        &database,
        code_exchange("code", &grant, "access", "refresh"),
        1,
    )
    .await;
    assert!(exchanged.is_some());
    assert!(authenticate(&database, "access", &grant, 2).await.is_some());
    assert!(
        database
            .register_mcp_client_bounded(
                &oauth_client("cleanup-trigger", "Cleanup trigger"),
                UnixMillis::new(200),
                10,
            )
            .await
            .expect("cleanup while token family is active")
    );

    let replay = exchange(
        &database,
        code_exchange("code", &grant, "attacker-access", "attacker-refresh"),
        201,
    )
    .await;
    assert!(replay.is_none());
    assert!(
        authenticate(&database, "access", &grant, 202)
            .await
            .is_none()
    );
    assert!(matches!(
        rotate(
            &database,
            refresh_rotation("refresh", &grant, "next-access", "next-refresh"),
            203,
        )
        .await,
        McpRefreshTokenRotationOutcome::InvalidToken
    ));
}

#[tokio::test]
async fn sqlite_repository_satisfies_the_shared_contract() {
    let database = database().await;
    mcp_authorization_server::testkit::assert_repository_contract(std::sync::Arc::new(database))
        .await;
}

#[tokio::test]
async fn token_issuance_failure_rolls_back_authorization_code_consumption() {
    let database = database().await;
    let client = oauth_client("client", "Client");
    database
        .upsert_mcp_client(&client, UnixMillis::new(0))
        .await
        .expect("client");
    let grant = grant(&client, &["notes:read"]);
    for code in ["first-code", "retryable-code"] {
        issue_code(
            &database,
            code,
            &client,
            McpClientRegistrationMethod::Dynamic,
            &grant,
            1_000,
            0,
        )
        .await;
    }
    exchange(
        &database,
        code_exchange("first-code", &grant, "colliding-access", "first-refresh"),
        1,
    )
    .await
    .expect("first grant");

    let failed = database
        .exchange_mcp_authorization_code(
            code_exchange(
                "retryable-code",
                &grant,
                "colliding-access",
                "failed-refresh",
            ),
            UnixMillis::new(2),
        )
        .await;
    assert!(matches!(failed, Err(SqliteStoreError::Database(_))));

    let retried = exchange(
        &database,
        code_exchange("retryable-code", &grant, "retry-access", "retry-refresh"),
        3,
    )
    .await;
    assert!(retried.is_some());
}
