use marginalis_server::ServerConfig;
use marginalis_sqlite::SqliteDatabase;
use marginalis_web::{ApiState, OidcAuthentication, OidcConfiguration, router};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (configuration, secrets) = ServerConfig::from_environment()?;
    std::fs::create_dir_all(&configuration.data_dir)?;
    let database = SqliteDatabase::connect(&configuration.database_url).await?;
    // root初期化は次の移行単位でRootCredential portへ移す。
    let _ = secrets.initial_root_password;
    let oidc_configuration = OidcConfiguration::new(
        configuration.oidc.issuer_url.to_string(),
        configuration.oidc.client_id,
        secrets.oidc_client_secret,
        configuration.base_url.as_str(),
    )?;
    let oidc = OidcAuthentication::discover(&oidc_configuration).await?;
    let listener = tokio::net::TcpListener::bind(configuration.listen_address).await?;
    axum::serve(listener, router(ApiState::with_oidc(database, oidc))).await?;
    Ok(())
}
