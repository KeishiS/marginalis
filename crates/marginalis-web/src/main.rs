use std::env;

use marginalis_store::NotebookStore;
use marginalis_web::{ApiState, OidcAuthentication, OidcConfiguration, router};
use time::{OffsetDateTime, UtcOffset, macros::format_description};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let database_url = required("MARGINALIS_DATABASE_URL")?;
    let base_url = required("MARGINALIS_BASE_URL")?;
    let store = NotebookStore::connect(&database_url).await?;
    if !store.root_is_initialized().await? {
        let password = required("ROOT_PASSWORD")?;
        let now =
            OffsetDateTime::now_utc()
                .to_offset(UtcOffset::UTC)
                .format(format_description!(
                    "[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3]Z"
                ))?;
        store.initialize_root(&password, &now).await?;
    }
    let oidc =
        OidcAuthentication::discover(&OidcConfiguration::from_environment(&base_url)?).await?;
    let address = required("MARGINALIS_LISTEN_ADDR")?;
    let listener = tokio::net::TcpListener::bind(address).await?;
    axum::serve(listener, router(ApiState::with_oidc(store, oidc))).await?;
    Ok(())
}

fn required(variable: &'static str) -> Result<String, std::io::Error> {
    match env::var(variable) {
        Ok(value) if !value.is_empty() => Ok(value),
        _ => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("required environment variable {variable} is not set"),
        )),
    }
}
