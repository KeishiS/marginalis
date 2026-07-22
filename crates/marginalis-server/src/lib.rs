//! サーバーの設定境界。環境変数とNixOS moduleはこの型へ変換される。

use core::fmt;
use std::{env, net::SocketAddr, path::PathBuf};

use url::Url;

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
            oidc_client_secret: required("OIDC_CLIENT_SECRET")?,
            initial_root_password: env::var("ROOT_PASSWORD")
                .ok()
                .filter(|value| !value.is_empty()),
        };
        Ok((configuration, secrets))
    }
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
