//! サーバーの設定境界。環境変数とNixOS moduleはこの型へ変換される。

use core::fmt;
use std::{env, net::SocketAddr, path::PathBuf};

use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use marginalis_application::{
    AuthenticationUseCaseError, Clock, NoteAclService, NoteAclServiceError, NoteAclStore,
    NoteOperationKind, NoteQueryStore, NoteUseCaseError, NoteUseCases, NoteWriteService,
    OidcUserAdministrationStore, Random, RootCredentialStore, SessionLifetime,
    WebAuthenticationUseCases, WebSession, WebSessionService, WebSessionStore,
};
use marginalis_auth_oidc::{OidcAuthentication, OidcCallbackError};
use marginalis_domain::{
    Actor, EntityId, NoteId, NotePermission, NoteSource, NoteSummary, OidcLoginResult,
    SourceRevision, UnixMillis, UserId,
};
use marginalis_files::FileNoteStore;
use marginalis_sqlite::SqliteDatabase;
use url::Url;
use uuid::Uuid;

/// server組立時に使うUTC millisecond clock。
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> UnixMillis {
        UnixMillis::new(time::OffsetDateTime::now_utc().unix_timestamp_nanos() as i64 / 1_000_000)
    }
}

/// UUIDv7と暗号学的に安全な不透明tokenを生成する実行環境adapter。
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemRandom;

impl Random for SystemRandom {
    fn uuid_v7(&self) -> EntityId {
        EntityId::from_uuid_v7(Uuid::now_v7())
    }

    fn opaque_token(&self) -> String {
        let bytes: [u8; 32] = rand::random();
        URL_SAFE_NO_PAD.encode(bytes)
    }
}

/// adapter群を組み合わせて、transportへノート操作だけを公開するserver側実装。
#[derive(Clone, Debug)]
pub struct ServerNoteUseCases {
    database: SqliteDatabase,
    sources: FileNoteStore,
}

/// Web session、外部OIDCとroot管理を同じapplication境界で公開するserver adapter。
#[derive(Clone)]
pub struct ServerWebAuthenticationUseCases {
    database: SqliteDatabase,
    oidc: Option<OidcAuthentication>,
}

impl ServerWebAuthenticationUseCases {
    pub const fn new(database: SqliteDatabase) -> Self {
        Self {
            database,
            oidc: None,
        }
    }

    pub fn with_oidc(database: SqliteDatabase, oidc: OidcAuthentication) -> Self {
        Self {
            database,
            oidc: Some(oidc),
        }
    }

    fn oidc(&self) -> Result<&OidcAuthentication, AuthenticationUseCaseError> {
        self.oidc
            .as_ref()
            .ok_or(AuthenticationUseCaseError::Unavailable)
    }
}

#[async_trait]
impl WebAuthenticationUseCases for ServerWebAuthenticationUseCases {
    async fn begin_oidc_login(&self) -> Result<String, AuthenticationUseCaseError> {
        self.oidc()?
            .begin_login(
                &self.database.oidc_login_attempt_store(),
                &SystemRandom,
                &SystemClock,
            )
            .await
            .map_err(|_| AuthenticationUseCaseError::Unavailable)
    }

    async fn complete_oidc_login(
        &self,
        code: String,
        state: String,
    ) -> Result<OidcLoginResult, AuthenticationUseCaseError> {
        self.oidc()?
            .complete_login(
                &self.database.oidc_login_attempt_store(),
                &self.database.oidc_identity_store(),
                &SystemRandom,
                &SystemClock,
                &code,
                &state,
            )
            .await
            .map_err(|error| match error {
                OidcCallbackError::Rejected(_) => AuthenticationUseCaseError::Rejected,
                OidcCallbackError::Unavailable => AuthenticationUseCaseError::Unavailable,
            })
    }

    async fn authenticate_session(
        &self,
        session_id: String,
    ) -> Result<Option<marginalis_application::AuthenticatedSession>, AuthenticationUseCaseError>
    {
        self.database
            .web_session_store()
            .lookup(session_id, SystemClock.now())
            .await
            .map_err(|_| AuthenticationUseCaseError::Unavailable)
    }

    async fn verify_csrf(
        &self,
        session_id: String,
        csrf_token: String,
    ) -> Result<bool, AuthenticationUseCaseError> {
        self.database
            .web_session_store()
            .verify_csrf(session_id, csrf_token, SystemClock.now())
            .await
            .map_err(|_| AuthenticationUseCaseError::Unavailable)
    }

    async fn issue_oidc_session(
        &self,
        user_id: UserId,
    ) -> Result<WebSession, AuthenticationUseCaseError> {
        WebSessionService::new(
            &self.database.web_session_store(),
            &SystemRandom,
            &SystemClock,
        )
        .issue(
            Actor {
                user_id,
                is_root: false,
            },
            SessionLifetime {
                idle_timeout_ms: 8 * 60 * 60 * 1_000,
                absolute_timeout_ms: 7 * 24 * 60 * 60 * 1_000,
            },
        )
        .await
        .map_err(|_| AuthenticationUseCaseError::Unavailable)
    }

    async fn root_login(
        &self,
        password: String,
    ) -> Result<Option<WebSession>, AuthenticationUseCaseError> {
        let Some(user_id) = self
            .database
            .root_credential_store()
            .verify_password(password)
            .await
            .map_err(|_| AuthenticationUseCaseError::Unavailable)?
        else {
            return Ok(None);
        };
        WebSessionService::new(
            &self.database.web_session_store(),
            &SystemRandom,
            &SystemClock,
        )
        .issue(
            Actor {
                user_id,
                is_root: true,
            },
            SessionLifetime {
                idle_timeout_ms: 30 * 60 * 1_000,
                absolute_timeout_ms: 8 * 60 * 60 * 1_000,
            },
        )
        .await
        .map(Some)
        .map_err(|_| AuthenticationUseCaseError::Unavailable)
    }

    async fn revoke_session(&self, session_id: String) -> Result<(), AuthenticationUseCaseError> {
        self.database
            .web_session_store()
            .revoke(session_id, SystemClock.now())
            .await
            .map_err(|_| AuthenticationUseCaseError::Unavailable)
    }

    async fn list_pending_users(
        &self,
    ) -> Result<Vec<marginalis_domain::OidcUser>, AuthenticationUseCaseError> {
        self.database
            .oidc_user_administration_store()
            .list_pending()
            .await
            .map_err(|_| AuthenticationUseCaseError::Unavailable)
    }

    async fn activate_pending_user(
        &self,
        user_id: UserId,
    ) -> Result<bool, AuthenticationUseCaseError> {
        self.database
            .oidc_user_administration_store()
            .activate(user_id, SystemClock.now())
            .await
            .map_err(|_| AuthenticationUseCaseError::Unavailable)
    }

    fn cookie_path(&self) -> &str {
        self.oidc
            .as_ref()
            .map_or("/", OidcAuthentication::cookie_path)
    }
}

impl ServerNoteUseCases {
    pub const fn new(database: SqliteDatabase, sources: FileNoteStore) -> Self {
        Self { database, sources }
    }

    pub async fn recover(&self) -> Result<(), NoteUseCaseError> {
        let projections = self.database.note_projection_store();
        let journal = self.database.operation_journal();
        NoteWriteService::new(
            &self.sources,
            &projections,
            &journal,
            &SystemRandom,
            &SystemClock,
        )
        .recover()
        .await
        .map_err(|_| NoteUseCaseError::Unavailable)
    }
}

#[async_trait]
impl NoteUseCases for ServerNoteUseCases {
    async fn list_notes(
        &self,
        actor: Actor,
        limit: u32,
    ) -> Result<Vec<NoteSummary>, NoteUseCaseError> {
        self.database
            .note_query_store()
            .list_visible(actor, limit)
            .await
            .map_err(|_| NoteUseCaseError::Unavailable)
    }

    async fn search_notes(
        &self,
        actor: Actor,
        query: String,
        limit: u32,
    ) -> Result<Vec<NoteSummary>, NoteUseCaseError> {
        if query.trim().is_empty() {
            return Err(NoteUseCaseError::Validation);
        }
        self.database
            .note_query_store()
            .search_visible(actor, query, limit)
            .await
            .map_err(|_| NoteUseCaseError::Unavailable)
    }

    async fn read_source(
        &self,
        actor: Actor,
        note_id: NoteId,
    ) -> Result<NoteSource, NoteUseCaseError> {
        let permission = self
            .database
            .note_acl_store()
            .permission_for(actor, note_id)
            .await
            .map_err(|_| NoteUseCaseError::Unavailable)?;
        if !matches!(permission, Some(value) if value.permits(NotePermission::Read)) {
            return Err(NoteUseCaseError::NotFound);
        }
        let content = self
            .sources
            .read(note_id)
            .map_err(|_| NoteUseCaseError::Unavailable)?
            .ok_or(NoteUseCaseError::NotFound)?;
        Ok(NoteSource {
            revision: SourceRevision::from_source(&content),
            content,
        })
    }

    async fn create_source(
        &self,
        actor: Actor,
        source: String,
    ) -> Result<NoteId, NoteUseCaseError> {
        let projection = marginalis_asciidoc::parse_note_projection(&source)
            .map_err(|_| NoteUseCaseError::Validation)?;
        if projection.owner_id != actor.user_id {
            return Err(NoteUseCaseError::Forbidden);
        }
        if self
            .sources
            .read(projection.note_id)
            .map_err(|_| NoteUseCaseError::Unavailable)?
            .is_some()
        {
            return Err(NoteUseCaseError::Conflict);
        }
        let note_id = projection.note_id;
        let projections = self.database.note_projection_store();
        let journal = self.database.operation_journal();
        NoteWriteService::new(
            &self.sources,
            &projections,
            &journal,
            &SystemRandom,
            &SystemClock,
        )
        .replace(NoteOperationKind::Create, projection, source.into_bytes())
        .await
        .map_err(|_| NoteUseCaseError::Unavailable)?;
        Ok(note_id)
    }

    async fn update_source(
        &self,
        actor: Actor,
        note_id: NoteId,
        source: String,
        expected_revision: SourceRevision,
    ) -> Result<(), NoteUseCaseError> {
        let permission = self
            .database
            .note_acl_store()
            .permission_for(actor, note_id)
            .await
            .map_err(|_| NoteUseCaseError::Unavailable)?;
        if !matches!(permission, Some(value) if value.permits(NotePermission::Write)) {
            return Err(NoteUseCaseError::NotFound);
        }
        let projection = marginalis_asciidoc::parse_note_projection(&source)
            .map_err(|_| NoteUseCaseError::Validation)?;
        if projection.note_id != note_id {
            return Err(NoteUseCaseError::Validation);
        }
        let previous_source = self
            .sources
            .read(note_id)
            .map_err(|_| NoteUseCaseError::Unavailable)?
            .ok_or(NoteUseCaseError::NotFound)?;
        if SourceRevision::from_source(&previous_source) != expected_revision {
            return Err(NoteUseCaseError::Conflict);
        }
        let previous_source =
            std::str::from_utf8(&previous_source).map_err(|_| NoteUseCaseError::Unavailable)?;
        let previous_projection = marginalis_asciidoc::parse_note_projection(previous_source)
            .map_err(|_| NoteUseCaseError::Unavailable)?;
        if projection.owner_id != previous_projection.owner_id {
            return Err(NoteUseCaseError::Validation);
        }
        let projections = self.database.note_projection_store();
        let journal = self.database.operation_journal();
        NoteWriteService::new(
            &self.sources,
            &projections,
            &journal,
            &SystemRandom,
            &SystemClock,
        )
        .replace(NoteOperationKind::Update, projection, source.into_bytes())
        .await
        .map(|_| ())
        .map_err(|_| NoteUseCaseError::Unavailable)
    }

    async fn delete_note(
        &self,
        actor: Actor,
        note_id: NoteId,
        expected_revision: SourceRevision,
    ) -> Result<(), NoteUseCaseError> {
        let permission = self
            .database
            .note_acl_store()
            .permission_for(actor, note_id)
            .await
            .map_err(|_| NoteUseCaseError::Unavailable)?;
        if !matches!(permission, Some(value) if value.permits(NotePermission::Admin)) {
            return Err(NoteUseCaseError::NotFound);
        }
        let source = self
            .sources
            .read(note_id)
            .map_err(|_| NoteUseCaseError::Unavailable)?
            .ok_or(NoteUseCaseError::NotFound)?;
        if SourceRevision::from_source(&source) != expected_revision {
            return Err(NoteUseCaseError::Conflict);
        }
        let projections = self.database.note_projection_store();
        let journal = self.database.operation_journal();
        NoteWriteService::new(
            &self.sources,
            &projections,
            &journal,
            &SystemRandom,
            &SystemClock,
        )
        .delete(note_id)
        .await
        .map_err(|_| NoteUseCaseError::Unavailable)
    }

    async fn set_permission(
        &self,
        actor: Actor,
        note_id: NoteId,
        user_id: UserId,
        permission: Option<NotePermission>,
    ) -> Result<(), NoteUseCaseError> {
        NoteAclService::new(&self.database.note_acl_store())
            .set_permission(actor, note_id, user_id, permission)
            .await
            .map_err(|error| match error {
                NoteAclServiceError::Forbidden => NoteUseCaseError::Forbidden,
                NoteAclServiceError::Store(_) => NoteUseCaseError::Conflict,
            })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServerConfig {
    pub base_url: Url,
    pub listen_address: SocketAddr,
    pub data_dir: PathBuf,
    pub database_url: String,
    pub oidc: OidcPublicConfig,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OidcPublicConfig {
    pub issuer_url: Url,
    pub client_id: String,
}

/// secret値は公開設定から分離する。Debugを実装せずログ出力を防ぐ。
pub struct SecretConfig {
    pub oidc_client_secret: String,
    pub initial_root_password: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConfigurationError {
    MissingEnvironment(&'static str),
    InvalidBaseUrl,
    InvalidIssuerUrl,
    InvalidListenAddress,
    EmptyClientId,
    EmptyDataDirectory,
    UnreadableSecretFile(&'static str),
}

impl fmt::Display for ConfigurationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingEnvironment(name) => {
                write!(formatter, "required environment variable {name} is not set")
            }
            Self::InvalidBaseUrl => formatter.write_str(
                "MARGINALIS_BASE_URL must be an absolute HTTPS URL without query or fragment",
            ),
            Self::InvalidIssuerUrl => {
                formatter.write_str("OIDC_ISSUER_URL must be an absolute HTTPS URL")
            }
            Self::InvalidListenAddress => formatter.write_str("MARGINALIS_LISTEN_ADDR is invalid"),
            Self::EmptyClientId => formatter.write_str("OIDC_CLIENT_ID must not be empty"),
            Self::EmptyDataDirectory => {
                formatter.write_str("MARGINALIS_DATA_DIR must not be empty")
            }
            Self::UnreadableSecretFile(name) => {
                write!(formatter, "secret file for {name} could not be read")
            }
        }
    }
}

impl std::error::Error for ConfigurationError {}

impl ServerConfig {
    pub fn from_environment() -> Result<(Self, SecretConfig), ConfigurationError> {
        let base_url = validate_base_url(required("MARGINALIS_BASE_URL")?)?;
        let issuer_url = validate_issuer_url(required("OIDC_ISSUER_URL")?)?;
        let client_id = required("OIDC_CLIENT_ID")?;
        if client_id.is_empty() {
            return Err(ConfigurationError::EmptyClientId);
        }
        let data_dir = PathBuf::from(required("MARGINALIS_DATA_DIR")?);
        if data_dir.as_os_str().is_empty() {
            return Err(ConfigurationError::EmptyDataDirectory);
        }
        let listen_address = required("MARGINALIS_LISTEN_ADDR")?
            .parse()
            .map_err(|_| ConfigurationError::InvalidListenAddress)?;
        let configuration = Self {
            base_url,
            listen_address,
            data_dir,
            database_url: required("MARGINALIS_DATABASE_URL")?,
            oidc: OidcPublicConfig {
                issuer_url,
                client_id,
            },
        };
        let secrets = SecretConfig {
            oidc_client_secret: required_secret("OIDC_CLIENT_SECRET")?,
            initial_root_password: optional_secret("ROOT_PASSWORD")?,
        };
        Ok((configuration, secrets))
    }
}

fn required_secret(name: &'static str) -> Result<String, ConfigurationError> {
    optional_secret(name)?.ok_or(ConfigurationError::MissingEnvironment(name))
}

fn optional_secret(name: &'static str) -> Result<Option<String>, ConfigurationError> {
    let file_variable = format!("{name}_FILE");
    if let Some(path) = env::var_os(file_variable) {
        let value = std::fs::read_to_string(path)
            .map_err(|_| ConfigurationError::UnreadableSecretFile(name))?
            .trim_end_matches(['\r', '\n'])
            .to_owned();
        return (!value.is_empty())
            .then_some(value)
            .ok_or(ConfigurationError::MissingEnvironment(name))
            .map(Some);
    }
    Ok(env::var(name).ok().filter(|value| !value.is_empty()))
}

fn required(name: &'static str) -> Result<String, ConfigurationError> {
    env::var(name)
        .ok()
        .filter(|value| !value.is_empty())
        .ok_or(ConfigurationError::MissingEnvironment(name))
}

fn validate_base_url(value: String) -> Result<Url, ConfigurationError> {
    let url = Url::parse(&value).map_err(|_| ConfigurationError::InvalidBaseUrl)?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(ConfigurationError::InvalidBaseUrl);
    }
    Ok(url)
}

fn validate_issuer_url(value: String) -> Result<Url, ConfigurationError> {
    let url = Url::parse(&value).map_err(|_| ConfigurationError::InvalidIssuerUrl)?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(ConfigurationError::InvalidIssuerUrl);
    }
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_url_rejects_non_https() {
        assert_eq!(
            validate_base_url("http://example.test".into()),
            Err(ConfigurationError::InvalidBaseUrl)
        );
    }

    #[test]
    fn base_url_accepts_subpath() {
        assert_eq!(
            validate_base_url("https://example.test/marginalis".into())
                .expect("valid URL")
                .path(),
            "/marginalis"
        );
    }
}
