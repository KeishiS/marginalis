use marginalis_server::ServerConfig;
use marginalis_store::NotebookStore;
use marginalis_web::{ApiState, OidcAuthentication, OidcConfiguration, router};
use time::{OffsetDateTime, UtcOffset, macros::format_description};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (configuration, secrets) = ServerConfig::from_environment()?;
    std::fs::create_dir_all(&configuration.data_dir)?;
    let store = NotebookStore::connect(&configuration.database_url).await?;
    if !store.root_is_initialized().await? {
        let password = secrets.initial_root_password.as_deref().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "ROOT_PASSWORD or ROOT_PASSWORD_FILE is required for an uninitialized database",
            )
        })?;
        let now =
            OffsetDateTime::now_utc()
                .to_offset(UtcOffset::UTC)
                .format(format_description!(
                    "[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3]Z"
                ))?;
        store.initialize_root(password, &now).await?;
    }
    let oidc_configuration = OidcConfiguration::new(
        configuration.oidc.issuer_url.to_string(),
        configuration.oidc.client_id,
        secrets.oidc_client_secret,
        configuration.base_url.as_str(),
    )?;
    let oidc = OidcAuthentication::discover(&oidc_configuration).await?;
    let listener = tokio::net::TcpListener::bind(configuration.listen_address).await?;
    axum::serve(listener, router(ApiState::with_oidc(store, oidc))).await?;
    Ok(())
}
