use marginalis_application::{Clock, Random, RootInitializationService};
use marginalis_domain::{EntityId, UnixMillis};
use marginalis_server::ServerConfig;
use marginalis_sqlite::SqliteDatabase;
use marginalis_web::{ApiState, OidcAuthentication, OidcConfiguration, router};
use uuid::Uuid;

struct ClockImpl;
impl Clock for ClockImpl {
    fn now(&self) -> UnixMillis {
        UnixMillis::new(time::OffsetDateTime::now_utc().unix_timestamp_nanos() as i64 / 1_000_000)
    }
}
struct RandomImpl;
impl Random for RandomImpl {
    fn uuid_v7(&self) -> EntityId {
        EntityId::from_uuid_v7(Uuid::now_v7())
    }
    fn opaque_token(&self) -> String {
        String::new()
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (configuration, secrets) = ServerConfig::from_environment()?;
    std::fs::create_dir_all(&configuration.data_dir)?;
    let database = SqliteDatabase::connect(&configuration.database_url).await?;
    let password = secrets
        .initial_root_password
        .ok_or("ROOT_PASSWORD or ROOT_PASSWORD_FILE is required for an uninitialized database")?;
    RootInitializationService::new(&database.root_credential_store(), &RandomImpl, &ClockImpl)
        .initialize_if_missing(password)
        .await?;
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
