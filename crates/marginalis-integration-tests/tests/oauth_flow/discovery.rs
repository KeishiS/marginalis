#[tokio::test]
async fn oidc_discovery_is_retried_with_the_configured_http_client() {
    let idp = MockIdentityProvider::start(CLIENT_ID, CLIENT_SECRET).await;
    let database = SqliteDatabase::connect("sqlite::memory:")
        .await
        .expect("database");
    let configuration = OidcConfiguration::new(
        idp.issuer,
        CLIENT_ID.into(),
        CLIENT_SECRET.into(),
        BROWSER_ORIGIN,
    )
    .expect("OIDC configuration");
    let provider = OidcIdentityProvider::new(
        database.oidc_login_attempt_store(),
        SystemClock,
        SystemRandom,
        configuration,
        reqwest::Client::new(),
        None,
    );
    let authentication = OidcAuthenticationApplication::new(Arc::new(provider), "server-users");

    assert!(authentication.begin_login().await.is_ok());
}
