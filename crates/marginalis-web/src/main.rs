use marginalis_application::{NoteWriteService, RootCredentialStore, RootInitializationService};
use marginalis_files::FileNoteStore;
use marginalis_server::{ServerConfig, SystemClock, SystemRandom};
use marginalis_sqlite::SqliteDatabase;
use marginalis_web::{ApiState, OidcAuthentication, OidcConfiguration, router};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    initialize_tracing();
    if let Err(error) = run().await {
        tracing::error!(error = %error, "Marginalis server terminated");
        std::process::exit(1);
    }
}

fn initialize_tracing() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,marginalis_auth_oidc=info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .compact()
        .init();
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let (configuration, secrets) = ServerConfig::from_environment()?;
    std::fs::create_dir_all(&configuration.data_dir)?;
    let database = SqliteDatabase::connect(&configuration.database_url).await?;
    let sources = FileNoteStore::open(&configuration.data_dir)?;
    let projections = database.note_projection_store();
    let journal = database.operation_journal();
    NoteWriteService::new(
        &sources,
        &projections,
        &journal,
        &SystemRandom,
        &SystemClock,
    )
    .recover()
    .await?;
    let root_store = database.root_credential_store();
    if !root_store.is_initialized().await? {
        let password = secrets.initial_root_password.ok_or(
            "ROOT_PASSWORD or ROOT_PASSWORD_FILE is required for an uninitialized database",
        )?;
        RootInitializationService::new(&root_store, &SystemRandom, &SystemClock)
            .initialize_if_missing(password)
            .await?;
    }
    let oidc_configuration = OidcConfiguration::new(
        configuration.oidc.issuer_url.to_string(),
        configuration.oidc.client_id,
        secrets.oidc_client_secret,
        configuration.base_url.as_str(),
    )?;
    let oidc = OidcAuthentication::discover(&oidc_configuration).await?;
    let listener = tokio::net::TcpListener::bind(configuration.listen_address).await?;
    tracing::info!(address = %configuration.listen_address, "Marginalis server listening");
    axum::serve(
        listener,
        router(ApiState::with_oidc(database, sources, oidc)),
    )
    .await?;
    Ok(())
}
