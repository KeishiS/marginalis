//! [`Repository`]実装へ共通に適用する契約試験。

use std::sync::Arc;

use crate::{
    AuthorizationCodeExchange, AuthorizationGrant, Client, ClientRegistrationMethod, Principal,
    RefreshTokenRotation, RefreshTokenRotationOutcome, RegisteredClient, Repository,
    ResolvedRedirectUri, Timestamp,
};

/// 認可codeとtokenの原子性、期限、binding、取消に関する共通契約を確認する。
///
/// 呼び出し側は、この試験専用の空のrepositoryを渡す。
pub async fn assert_repository_contract(repository: Arc<dyn Repository>) {
    let client = test_client("contract-client");
    let registered = registered_client(&client);
    assert!(
        repository
            .register_client_bounded(&client, Timestamp::new(0), 1)
            .await
            .expect("register first client")
    );
    assert_eq!(
        repository
            .client(&client.client_id)
            .await
            .expect("look up client"),
        Some(registered.clone())
    );
    assert!(
        !repository
            .register_client_bounded(&test_client("over-capacity"), Timestamp::new(1), 1)
            .await
            .expect("enforce client bound")
    );

    let grant = authorization_grant(&client);
    repository
        .issue_authorization_code(
            "expired-code",
            &registered,
            &grant,
            "challenge",
            Timestamp::new(10),
            Timestamp::new(2),
        )
        .await
        .expect("issue expiring authorization code");
    assert!(
        repository
            .exchange_authorization_code(
                exchange(
                    "expired-code",
                    &grant,
                    "expired-access",
                    "expired-refresh",
                    30,
                    40
                ),
                Timestamp::new(10),
            )
            .await
            .expect("reject expired authorization code")
            .is_none()
    );

    repository
        .issue_authorization_code(
            "live-code",
            &registered,
            &grant,
            "challenge",
            Timestamp::new(20),
            Timestamp::new(11),
        )
        .await
        .expect("issue live authorization code");
    assert!(
        repository
            .exchange_authorization_code(
                exchange("live-code", &grant, "access", "refresh", 30, 40),
                Timestamp::new(19),
            )
            .await
            .expect("exchange live authorization code")
            .is_some()
    );
    assert!(
        repository
            .authenticate_access_token(
                "access",
                "https://different-resource.example/mcp",
                Timestamp::new(29),
            )
            .await
            .expect("reject access token for another resource")
            .is_none()
    );
    assert!(
        repository
            .authenticate_access_token("access", &grant.resource_uri, Timestamp::new(29))
            .await
            .expect("authenticate live access token")
            .is_some()
    );
    assert!(
        repository
            .authenticate_access_token("access", &grant.resource_uri, Timestamp::new(30))
            .await
            .expect("reject access token at expiry")
            .is_none()
    );
    assert_eq!(
        repository
            .rotate_refresh_token(
                rotation(
                    "refresh",
                    &grant,
                    "expired-next-access",
                    "expired-next-refresh",
                    50,
                    60
                ),
                Timestamp::new(40),
            )
            .await
            .expect("reject refresh token at expiry"),
        RefreshTokenRotationOutcome::InvalidToken
    );

    issue_and_exchange(
        repository.as_ref(),
        &registered,
        &grant,
        "rotation-code",
        "rotation-access",
        "rotation-refresh",
        50,
        70,
        80,
        49,
    )
    .await;
    assert!(matches!(
        repository
            .rotate_refresh_token(
                rotation(
                    "rotation-refresh",
                    &grant,
                    "next-access",
                    "next-refresh",
                    75,
                    90,
                ),
                Timestamp::new(60),
            )
            .await
            .expect("rotate refresh token"),
        RefreshTokenRotationOutcome::Rotated { .. }
    ));
    assert!(
        repository
            .authenticate_access_token("next-access", &grant.resource_uri, Timestamp::new(61))
            .await
            .expect("authenticate rotated access token")
            .is_some()
    );
    assert_eq!(
        repository
            .rotate_refresh_token(
                rotation(
                    "rotation-refresh",
                    &grant,
                    "replay-access",
                    "replay-refresh",
                    75,
                    90,
                ),
                Timestamp::new(62),
            )
            .await
            .expect("detect refresh token reuse"),
        RefreshTokenRotationOutcome::InvalidToken
    );
    assert!(
        repository
            .authenticate_access_token("next-access", &grant.resource_uri, Timestamp::new(63))
            .await
            .expect("reject family after refresh token reuse")
            .is_none()
    );
    assert_eq!(
        repository
            .rotate_refresh_token(
                rotation(
                    "next-refresh",
                    &grant,
                    "post-replay-access",
                    "post-replay-refresh",
                    80,
                    100,
                ),
                Timestamp::new(63),
            )
            .await
            .expect("reject refresh family after reuse"),
        RefreshTokenRotationOutcome::InvalidToken
    );

    issue_and_exchange(
        repository.as_ref(),
        &registered,
        &grant,
        "replay-code",
        "replay-family-access",
        "replay-family-refresh",
        100,
        120,
        140,
        64,
    )
    .await;
    assert!(
        repository
            .exchange_authorization_code(
                exchange(
                    "replay-code",
                    &grant,
                    "attacker-access",
                    "attacker-refresh",
                    120,
                    140,
                ),
                Timestamp::new(65),
            )
            .await
            .expect("detect authorization code reuse")
            .is_none()
    );
    assert!(
        repository
            .authenticate_access_token(
                "replay-family-access",
                &grant.resource_uri,
                Timestamp::new(66),
            )
            .await
            .expect("reject family after authorization code reuse")
            .is_none()
    );
    assert_eq!(
        repository
            .rotate_refresh_token(
                rotation(
                    "replay-family-refresh",
                    &grant,
                    "post-code-replay-access",
                    "post-code-replay-refresh",
                    130,
                    150,
                ),
                Timestamp::new(66),
            )
            .await
            .expect("reject refresh family after authorization code reuse"),
        RefreshTokenRotationOutcome::InvalidToken
    );

    repository
        .issue_authorization_code(
            "pending-code",
            &registered,
            &grant,
            "challenge",
            Timestamp::new(100),
            Timestamp::new(67),
        )
        .await
        .expect("issue pending authorization code");
    repository
        .revoke_client_tokens(
            grant.principal.issuer(),
            grant.principal.subject(),
            &grant.client_id,
            Timestamp::new(68),
        )
        .await
        .expect("revoke client grant");
    assert!(
        repository
            .exchange_authorization_code(
                exchange(
                    "pending-code",
                    &grant,
                    "late-access",
                    "late-refresh",
                    110,
                    120
                ),
                Timestamp::new(69),
            )
            .await
            .expect("reject authorization code after client revocation")
            .is_none()
    );

    let other_client = test_client("revocation-client");
    let other_registered = registered_client(&other_client);
    let other_grant = authorization_grant(&other_client);
    issue_and_exchange(
        repository.as_ref(),
        &other_registered,
        &other_grant,
        "revocation-code",
        "revocation-access",
        "revocation-refresh",
        100,
        120,
        140,
        70,
    )
    .await;
    repository
        .revoke_token("revocation-access", "different-client", Timestamp::new(71))
        .await
        .expect("hide token owned by another client");
    assert!(
        repository
            .authenticate_access_token(
                "revocation-access",
                &other_grant.resource_uri,
                Timestamp::new(72),
            )
            .await
            .expect("keep token after wrong-client revocation")
            .is_some()
    );
    repository
        .revoke_token(
            "revocation-access",
            &other_grant.client_id,
            Timestamp::new(73),
        )
        .await
        .expect("revoke token family");
    assert!(
        repository
            .authenticate_access_token(
                "revocation-access",
                &other_grant.resource_uri,
                Timestamp::new(74),
            )
            .await
            .expect("reject revoked access token")
            .is_none()
    );
}

fn test_client(client_id: &str) -> Client {
    Client {
        client_id: client_id.to_owned(),
        display_name: "Repository contract client".to_owned(),
        redirect_uris: vec!["https://client.example/callback".to_owned()],
    }
}

fn registered_client(client: &Client) -> RegisteredClient {
    RegisteredClient {
        client: client.clone(),
        registration_method: ClientRegistrationMethod::Dynamic,
    }
}

fn authorization_grant(client: &Client) -> AuthorizationGrant {
    AuthorizationGrant {
        principal: Principal::new("https://issuer.example".to_owned(), "alice".to_owned()),
        client_id: client.client_id.clone(),
        redirect_uri: ResolvedRedirectUri::Supplied(client.redirect_uris[0].clone()),
        resource_uri: "https://resource.example/mcp".to_owned(),
        scopes: vec!["items:read".to_owned()],
    }
}

fn exchange(
    code: &str,
    grant: &AuthorizationGrant,
    access_token: &str,
    refresh_token: &str,
    access_expires_at: i64,
    refresh_expires_at: i64,
) -> AuthorizationCodeExchange {
    AuthorizationCodeExchange {
        code: code.to_owned(),
        client_id: grant.client_id.clone(),
        redirect_uri: Some(grant.redirect_uri.as_str().to_owned()),
        resource_uri: grant.resource_uri.clone(),
        code_challenge: "challenge".to_owned(),
        access_token: access_token.to_owned(),
        refresh_token: refresh_token.to_owned(),
        access_expires_at: Timestamp::new(access_expires_at),
        refresh_expires_at: Timestamp::new(refresh_expires_at),
    }
}

fn rotation(
    refresh_token: &str,
    grant: &AuthorizationGrant,
    new_access_token: &str,
    new_refresh_token: &str,
    access_expires_at: i64,
    refresh_expires_at: i64,
) -> RefreshTokenRotation {
    RefreshTokenRotation {
        refresh_token: refresh_token.to_owned(),
        client_id: grant.client_id.clone(),
        resource_uri: grant.resource_uri.clone(),
        requested_scopes: None,
        new_access_token: new_access_token.to_owned(),
        new_refresh_token: new_refresh_token.to_owned(),
        access_expires_at: Timestamp::new(access_expires_at),
        refresh_expires_at: Timestamp::new(refresh_expires_at),
    }
}

#[allow(clippy::too_many_arguments)]
async fn issue_and_exchange(
    repository: &dyn Repository,
    registered: &RegisteredClient,
    grant: &AuthorizationGrant,
    code: &str,
    access_token: &str,
    refresh_token: &str,
    code_expires_at: i64,
    access_expires_at: i64,
    refresh_expires_at: i64,
    now: i64,
) {
    repository
        .issue_authorization_code(
            code,
            registered,
            grant,
            "challenge",
            Timestamp::new(code_expires_at),
            Timestamp::new(now - 1),
        )
        .await
        .expect("issue authorization code");
    assert!(
        repository
            .exchange_authorization_code(
                exchange(
                    code,
                    grant,
                    access_token,
                    refresh_token,
                    access_expires_at,
                    refresh_expires_at,
                ),
                Timestamp::new(now),
            )
            .await
            .expect("exchange authorization code")
            .is_some()
    );
}
