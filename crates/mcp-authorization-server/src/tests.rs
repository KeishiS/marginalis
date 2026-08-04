use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use async_trait::async_trait;

use crate::{
    AuthenticatedPrincipal, AuthorizationCodeExchange, AuthorizationError, AuthorizationGrant,
    AuthorizationRequest, AuthorizationServer, AuthorizationServerConfig, Client,
    ClientMetadataResolver, ClientMetadataResolverError, ClientRegistrationMethod, Clock,
    Principal, Random, RefreshTokenRotation, RefreshTokenRotationOutcome, RegisteredClient,
    Repository, RepositoryError, ResourcePolicy, Timestamp, pkce_s256,
};

#[derive(Default)]
struct MemoryState {
    clients: HashMap<String, RegisteredClient>,
    codes: HashMap<String, StoredCode>,
    access_tokens: HashMap<String, StoredAccessToken>,
    refresh_tokens: HashMap<String, StoredRefreshToken>,
    next_family: u64,
}

struct StoredCode {
    grant: AuthorizationGrant,
    code_challenge: String,
    expires_at: Timestamp,
    consumed_family: Option<u64>,
}

struct StoredAccessToken {
    grant: AuthorizationGrant,
    family: u64,
    expires_at: Timestamp,
    revoked: bool,
}

struct StoredRefreshToken {
    grant: AuthorizationGrant,
    family: u64,
    expires_at: Timestamp,
    consumed: bool,
}

#[derive(Default)]
struct MemoryRepository(Mutex<MemoryState>);

impl MemoryRepository {
    fn revoke_family(state: &mut MemoryState, family: u64) {
        for token in state.access_tokens.values_mut() {
            if token.family == family {
                token.revoked = true;
            }
        }
        for token in state.refresh_tokens.values_mut() {
            if token.family == family {
                token.consumed = true;
            }
        }
    }
}

#[async_trait]
impl Repository for MemoryRepository {
    async fn register_client_bounded(
        &self,
        client: &Client,
        _now: Timestamp,
        maximum_clients: i64,
    ) -> Result<bool, RepositoryError> {
        let mut state = self.0.lock().expect("memory repository");
        if !state.clients.contains_key(&client.client_id)
            && state.clients.len() >= usize::try_from(maximum_clients).unwrap_or(0)
        {
            return Ok(false);
        }
        state.clients.insert(
            client.client_id.clone(),
            RegisteredClient {
                client: client.clone(),
                registration_method: ClientRegistrationMethod::Dynamic,
            },
        );
        Ok(true)
    }

    async fn client(&self, client_id: &str) -> Result<Option<RegisteredClient>, RepositoryError> {
        Ok(self
            .0
            .lock()
            .expect("memory repository")
            .clients
            .get(client_id)
            .cloned())
    }

    async fn issue_authorization_code(
        &self,
        code: &str,
        client: &RegisteredClient,
        grant: &AuthorizationGrant,
        code_challenge: &str,
        expires_at: Timestamp,
        _now: Timestamp,
    ) -> Result<(), RepositoryError> {
        let mut state = self.0.lock().expect("memory repository");
        state
            .clients
            .insert(client.client.client_id.clone(), client.clone());
        state.codes.insert(
            code.to_owned(),
            StoredCode {
                grant: grant.clone(),
                code_challenge: code_challenge.to_owned(),
                expires_at,
                consumed_family: None,
            },
        );
        Ok(())
    }

    async fn exchange_authorization_code(
        &self,
        exchange: AuthorizationCodeExchange,
        now: Timestamp,
    ) -> Result<Option<AuthorizationGrant>, RepositoryError> {
        let mut state = self.0.lock().expect("memory repository");
        let Some(stored) = state.codes.get(&exchange.code) else {
            return Ok(None);
        };
        if let Some(family) = stored.consumed_family {
            Self::revoke_family(&mut state, family);
            return Ok(None);
        }
        let redirect_matches = exchange
            .redirect_uri
            .as_deref()
            .is_none_or(|uri| uri == stored.grant.redirect_uri.as_str());
        if stored.expires_at < now
            || stored.grant.client_id != exchange.client_id
            || stored.grant.resource_uri != exchange.resource_uri
            || stored.code_challenge != exchange.code_challenge
            || !redirect_matches
        {
            return Ok(None);
        }
        let grant = stored.grant.clone();
        state.next_family += 1;
        let family = state.next_family;
        state
            .codes
            .get_mut(&exchange.code)
            .expect("stored code")
            .consumed_family = Some(family);
        state.access_tokens.insert(
            exchange.access_token,
            StoredAccessToken {
                grant: grant.clone(),
                family,
                expires_at: exchange.access_expires_at,
                revoked: false,
            },
        );
        state.refresh_tokens.insert(
            exchange.refresh_token,
            StoredRefreshToken {
                grant: grant.clone(),
                family,
                expires_at: exchange.refresh_expires_at,
                consumed: false,
            },
        );
        Ok(Some(grant))
    }

    async fn rotate_refresh_token(
        &self,
        rotation: RefreshTokenRotation,
        now: Timestamp,
    ) -> Result<RefreshTokenRotationOutcome, RepositoryError> {
        let mut state = self.0.lock().expect("memory repository");
        let Some(stored) = state.refresh_tokens.get(&rotation.refresh_token) else {
            return Ok(RefreshTokenRotationOutcome::InvalidToken);
        };
        let family = stored.family;
        if stored.consumed {
            Self::revoke_family(&mut state, family);
            return Ok(RefreshTokenRotationOutcome::InvalidToken);
        }
        if stored.expires_at < now
            || stored.grant.client_id != rotation.client_id
            || stored.grant.resource_uri != rotation.resource_uri
        {
            return Ok(RefreshTokenRotationOutcome::InvalidToken);
        }
        let scopes = rotation
            .requested_scopes
            .clone()
            .unwrap_or_else(|| stored.grant.scopes.clone());
        if scopes
            .iter()
            .any(|scope| !stored.grant.scopes.contains(scope))
        {
            return Ok(RefreshTokenRotationOutcome::InvalidScope);
        }
        let mut grant = stored.grant.clone();
        grant.scopes.clone_from(&scopes);
        state
            .refresh_tokens
            .get_mut(&rotation.refresh_token)
            .expect("stored refresh token")
            .consumed = true;
        state.access_tokens.insert(
            rotation.new_access_token,
            StoredAccessToken {
                grant: grant.clone(),
                family,
                expires_at: rotation.access_expires_at,
                revoked: false,
            },
        );
        state.refresh_tokens.insert(
            rotation.new_refresh_token,
            StoredRefreshToken {
                grant,
                family,
                expires_at: rotation.refresh_expires_at,
                consumed: false,
            },
        );
        Ok(RefreshTokenRotationOutcome::Rotated {
            access_scopes: scopes,
        })
    }

    async fn authenticate_access_token(
        &self,
        token: &str,
        resource_uri: &str,
        now: Timestamp,
    ) -> Result<Option<AuthenticatedPrincipal>, RepositoryError> {
        Ok(self
            .0
            .lock()
            .expect("memory repository")
            .access_tokens
            .get(token)
            .filter(|stored| {
                !stored.revoked
                    && stored.expires_at >= now
                    && stored.grant.resource_uri == resource_uri
            })
            .map(|stored| AuthenticatedPrincipal {
                principal: stored.grant.principal.clone(),
                scopes: stored.grant.scopes.clone(),
            }))
    }

    async fn revoke_client_tokens(
        &self,
        issuer: &str,
        subject: &str,
        client_id: &str,
        _now: Timestamp,
    ) -> Result<(), RepositoryError> {
        let mut state = self.0.lock().expect("memory repository");
        let families = state
            .refresh_tokens
            .values()
            .filter(|stored| {
                stored.grant.principal.issuer() == issuer
                    && stored.grant.principal.subject() == subject
                    && stored.grant.client_id == client_id
            })
            .map(|stored| stored.family)
            .collect::<Vec<_>>();
        for family in families {
            Self::revoke_family(&mut state, family);
        }
        Ok(())
    }

    async fn revoke_token(
        &self,
        token: &str,
        client_id: &str,
        _now: Timestamp,
    ) -> Result<(), RepositoryError> {
        let mut state = self.0.lock().expect("memory repository");
        let family = state
            .refresh_tokens
            .get(token)
            .filter(|stored| stored.grant.client_id == client_id)
            .map(|stored| stored.family)
            .or_else(|| {
                state
                    .access_tokens
                    .get(token)
                    .filter(|stored| stored.grant.client_id == client_id)
                    .map(|stored| stored.family)
            });
        if let Some(family) = family {
            Self::revoke_family(&mut state, family);
        }
        Ok(())
    }
}

struct FixedClock(Timestamp);

impl Clock for FixedClock {
    fn now(&self) -> Timestamp {
        self.0
    }
}

#[derive(Default)]
struct SequentialRandom(Mutex<u32>);

impl Random for SequentialRandom {
    fn opaque_token(&self) -> String {
        let mut value = self.0.lock().expect("sequential random");
        *value += 1;
        format!("token-{value}")
    }
}

struct MissingMetadataResolver;

#[async_trait]
impl ClientMetadataResolver for MissingMetadataResolver {
    async fn resolve(
        &self,
        _client_id: &str,
    ) -> Result<Option<Client>, ClientMetadataResolverError> {
        Ok(None)
    }
}

fn test_server(repository: Arc<MemoryRepository>, resource_uri: &str) -> AuthorizationServer {
    AuthorizationServer::new(
        repository,
        Arc::new(FixedClock(Timestamp::new(1_000))),
        Arc::new(SequentialRandom::default()),
        ResourcePolicy::new(
            resource_uri.to_owned(),
            "Test resource".to_owned(),
            vec!["items:read".to_owned(), "items:write".to_owned()],
            vec!["items:read".to_owned()],
        )
        .expect("resource policy"),
        AuthorizationServerConfig::new(300, 3_600, 60, 10).expect("server config"),
    )
    .with_client_metadata_resolver(Arc::new(MissingMetadataResolver))
}

fn client() -> Client {
    Client {
        client_id: "test-client".to_owned(),
        display_name: "Test client".to_owned(),
        redirect_uris: vec!["https://client.example/callback".to_owned()],
    }
}

async fn issue_tokens(server: &AuthorizationServer) -> (crate::TokenPair, String) {
    let verifier = "a".repeat(43);
    let validated = server
        .validate_authorization_request(&AuthorizationRequest {
            client_id: "test-client".to_owned(),
            redirect_uri: Some("https://client.example/callback".to_owned()),
            resource_uri: "https://resource.example/mcp".to_owned(),
            scopes: vec!["items:read".to_owned(), "items:write".to_owned()],
            code_challenge: pkce_s256(&verifier),
        })
        .await
        .expect("validated request");
    let code = server
        .authorize(
            Principal::new("https://issuer.example".to_owned(), "alice".to_owned()),
            validated,
        )
        .await
        .expect("authorization code");
    let tokens = server
        .exchange_authorization_code(
            code.clone(),
            "test-client".to_owned(),
            Some("https://client.example/callback".to_owned()),
            "https://resource.example/mcp".to_owned(),
            verifier,
        )
        .await
        .expect("token pair");
    (tokens, code)
}

#[tokio::test]
async fn authorization_code_is_single_use_and_tokens_are_resource_bound() {
    let repository = Arc::new(MemoryRepository::default());
    let server = test_server(repository.clone(), "https://resource.example/mcp");
    server.register_client(client()).await.expect("client");

    let (tokens, code) = issue_tokens(&server).await;
    assert_eq!(tokens.access_expires_in_seconds, 300);
    let authenticated = server
        .authenticate(&tokens.access_token, "https://resource.example/mcp")
        .await
        .expect("authentication")
        .expect("principal");
    assert_eq!(authenticated.principal.subject(), "alice");
    assert_eq!(authenticated.scopes, ["items:read", "items:write"]);
    assert!(matches!(
        server
            .exchange_authorization_code(
                code,
                "test-client".to_owned(),
                Some("https://client.example/callback".to_owned()),
                "https://resource.example/mcp".to_owned(),
                "a".repeat(43),
            )
            .await,
        Err(AuthorizationError::InvalidGrant)
    ));
    assert!(
        server
            .authenticate(&tokens.access_token, "https://resource.example/mcp")
            .await
            .expect("authentication after code reuse")
            .is_none()
    );

    let other_resource = test_server(repository, "https://other.example/mcp");
    assert!(
        other_resource
            .authenticate(&tokens.access_token, "https://other.example/mcp")
            .await
            .expect("other resource authentication")
            .is_none()
    );
}

#[tokio::test]
async fn refresh_cannot_expand_scope_and_reuse_revokes_the_token_family() {
    let repository = Arc::new(MemoryRepository::default());
    let server = test_server(repository, "https://resource.example/mcp");
    server.register_client(client()).await.expect("client");
    let (tokens, _) = issue_tokens(&server).await;

    assert!(matches!(
        server
            .refresh_access_token(
                tokens.refresh_token.clone(),
                "test-client".to_owned(),
                "https://resource.example/mcp".to_owned(),
                Some(vec!["unsupported".to_owned()]),
            )
            .await,
        Err(AuthorizationError::InvalidScope)
    ));
    let rotated = server
        .refresh_access_token(
            tokens.refresh_token.clone(),
            "test-client".to_owned(),
            "https://resource.example/mcp".to_owned(),
            Some(vec!["items:read".to_owned()]),
        )
        .await
        .expect("rotated token");
    assert_eq!(rotated.scope, "items:read");

    assert!(matches!(
        server
            .refresh_access_token(
                tokens.refresh_token,
                "test-client".to_owned(),
                "https://resource.example/mcp".to_owned(),
                None,
            )
            .await,
        Err(AuthorizationError::InvalidGrant)
    ));
    assert!(
        server
            .authenticate(&rotated.access_token, "https://resource.example/mcp")
            .await
            .expect("authentication after reuse")
            .is_none()
    );
}

#[tokio::test]
async fn revocation_invalidates_all_tokens_for_the_client_grant() {
    let repository = Arc::new(MemoryRepository::default());
    let server = test_server(repository, "https://resource.example/mcp");
    server.register_client(client()).await.expect("client");
    let (tokens, _) = issue_tokens(&server).await;

    server
        .revoke(
            &Principal::new("https://issuer.example".to_owned(), "alice".to_owned()),
            "test-client",
        )
        .await
        .expect("revocation");
    assert!(
        server
            .authenticate(&tokens.access_token, "https://resource.example/mcp")
            .await
            .expect("authentication after revocation")
            .is_none()
    );
}

#[test]
fn server_configuration_rejects_unsafe_zero_values() {
    assert!(AuthorizationServerConfig::new(0, 3_600, 60, 10).is_none());
    assert!(AuthorizationServerConfig::new(300, 0, 60, 10).is_none());
    assert!(AuthorizationServerConfig::new(300, 3_600, 0, 10).is_none());
    assert!(AuthorizationServerConfig::new(300, 3_600, 60, 0).is_none());
}
