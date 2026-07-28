//! composition rootが環境変数から読み込む公開設定とsecret設定。

use core::fmt;
use std::{env, net::SocketAddr, path::PathBuf};

use url::Url;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServerConfig {
    pub http: HttpConfig,
    pub storage: StorageConfig,
    pub oidc: OidcConfig,
    pub mcp_enabled: bool,
    pub mcp_allowed_origins: Vec<String>,
}

/// HTTP transportだけが必要とする公開設定。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HttpConfig {
    pub base_url: Url,
    pub listen_address: SocketAddr,
}

/// SQLiteとAsciiDoc正本だけを扱うmaintenance command向けの設定境界。
///
/// backupはHTTP listener・OIDC client・secretを必要としないため、
/// `ServerConfig`を読まずこの型だけを利用する。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StorageConfig {
    pub database_url: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OidcConfig {
    pub issuer_url: Url,
    pub client_id: String,
    pub ca_certificate_file: Option<PathBuf>,
}

/// secret値は公開設定から分離する。Debugを実装せずログ出力を防ぐ。
pub struct SecretConfig {
    pub oidc_client_secret: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConfigurationError {
    MissingEnvironment(&'static str),
    InvalidBaseUrl,
    InvalidIssuerUrl,
    InvalidListenAddress,
    EmptyClientId,
    UnreadableSecretFile(&'static str),
    InvalidMcpEnable,
    InvalidMcpAllowedOrigin,
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
            Self::UnreadableSecretFile(name) => {
                write!(formatter, "secret file for {name} could not be read")
            }
            Self::InvalidMcpEnable => {
                formatter.write_str("MARGINALIS_MCP_ENABLE must be `true` or `false`")
            }
            Self::InvalidMcpAllowedOrigin => formatter.write_str(
                "MARGINALIS_MCP_ALLOWED_ORIGINS must contain comma-separated HTTPS origins",
            ),
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
        let storage = StorageConfig::from_environment()?;
        let listen_address = required("MARGINALIS_LISTEN_ADDR")?
            .parse()
            .map_err(|_| ConfigurationError::InvalidListenAddress)?;
        let configuration = Self {
            http: HttpConfig {
                base_url,
                listen_address,
            },
            storage,
            oidc: OidcConfig {
                issuer_url,
                client_id,
                ca_certificate_file: std::env::var_os("OIDC_CA_CERTIFICATE_FILE")
                    .filter(|value| !value.is_empty())
                    .map(PathBuf::from),
            },
            mcp_enabled: optional_bool("MARGINALIS_MCP_ENABLE")?.unwrap_or(false),
            mcp_allowed_origins: validate_mcp_allowed_origins(optional_csv(
                "MARGINALIS_MCP_ALLOWED_ORIGINS",
            )?)?,
        };
        let secrets = SecretConfig {
            oidc_client_secret: required_secret("OIDC_CLIENT_SECRET")?,
        };
        Ok((configuration, secrets))
    }
}

impl StorageConfig {
    pub fn from_environment() -> Result<Self, ConfigurationError> {
        Ok(Self {
            database_url: required("MARGINALIS_DATABASE_URL")?,
        })
    }
}

fn optional_bool(name: &'static str) -> Result<Option<bool>, ConfigurationError> {
    match env::var(name) {
        Ok(value) => match value.as_str() {
            "true" => Ok(Some(true)),
            "false" => Ok(Some(false)),
            _ => Err(ConfigurationError::InvalidMcpEnable),
        },
        Err(env::VarError::NotPresent) => Ok(None),
        Err(env::VarError::NotUnicode(_)) => Err(ConfigurationError::InvalidMcpEnable),
    }
}

fn optional_csv(name: &'static str) -> Result<Vec<String>, ConfigurationError> {
    match env::var(name) {
        Ok(value) => Ok(value
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .collect()),
        Err(env::VarError::NotPresent) => Ok(Vec::new()),
        Err(env::VarError::NotUnicode(_)) => Err(ConfigurationError::InvalidMcpAllowedOrigin),
    }
}

fn validate_mcp_allowed_origins(values: Vec<String>) -> Result<Vec<String>, ConfigurationError> {
    let mut origins = Vec::with_capacity(values.len());
    for value in values {
        let url = Url::parse(&value).map_err(|_| ConfigurationError::InvalidMcpAllowedOrigin)?;
        if url.scheme() != "https"
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.path() != "/"
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(ConfigurationError::InvalidMcpAllowedOrigin);
        }
        let origin = url.origin().ascii_serialization();
        if !origins.contains(&origin) {
            origins.push(origin);
        }
    }
    Ok(origins)
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
        || !url.username().is_empty()
        || url.password().is_some()
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

    #[test]
    fn issuer_url_rejects_userinfo() {
        for invalid in [
            "https://user@id.example.test",
            "https://user:password@id.example.test",
        ] {
            assert_eq!(
                validate_issuer_url(invalid.into()),
                Err(ConfigurationError::InvalidIssuerUrl)
            );
        }
    }

    #[test]
    fn mcp_allowed_origins_are_normalized_and_reject_non_origins() {
        assert_eq!(
            validate_mcp_allowed_origins(vec![
                "https://chatgpt.com".into(),
                "https://chatgpt.com".into(),
            ])
            .expect("origins"),
            vec!["https://chatgpt.com"]
        );
        for invalid in [
            "http://chatgpt.com",
            "https://chatgpt.com/path",
            "https://user@chatgpt.com",
            "not-an-origin",
        ] {
            assert_eq!(
                validate_mcp_allowed_origins(vec![invalid.into()]),
                Err(ConfigurationError::InvalidMcpAllowedOrigin)
            );
        }
    }
}
