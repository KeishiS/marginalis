//! composition rootが環境変数から読み込む公開設定とsecret設定。

use std::{net::SocketAddr, path::PathBuf};

use url::Url;

use crate::environment;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServerConfig {
    pub http: HttpConfig,
    pub storage: StorageConfig,
    pub oidc: OidcConfig,
    pub mcp: Option<McpConfig>,
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct McpAuthorizationConfig {
    pub issuer: String,
    pub upstream_issuer_claim: String,
    pub upstream_subject_claim: String,
    pub groups_claim: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct McpConfig {
    pub allowed_origins: Vec<String>,
    pub authorization: McpAuthorizationConfig,
}

/// secret値は公開設定から分離する。Debugを実装せずログ出力を防ぐ。
pub struct SecretConfig {
    pub oidc_client_secret: String,
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ConfigurationError {
    #[error("required environment variable {0} is not set")]
    MissingEnvironment(&'static str),
    #[error("{0} must be an absolute HTTPS URL without userinfo, query, or fragment")]
    InvalidHttpsUrl(&'static str),
    #[error("{} is invalid", environment::LISTEN_ADDRESS)]
    InvalidListenAddress,
    #[error("secret file for {0} could not be read")]
    UnreadableSecretFile(&'static str),
    #[error(
        "{} must contain comma-separated HTTPS origins",
        environment::MCP_ALLOWED_ORIGINS
    )]
    InvalidMcpAllowedOrigin,
}

impl ServerConfig {
    pub fn from_environment() -> Result<(Self, SecretConfig), ConfigurationError> {
        let base_url = validate_https_url(environment::BASE_URL)?;
        let issuer_url = validate_https_url(environment::OIDC_ISSUER_URL)?;
        let client_id = required(environment::OIDC_CLIENT_ID)?;
        let storage = StorageConfig::from_environment()?;
        let listen_address = required(environment::LISTEN_ADDRESS)?
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
                ca_certificate_file: environment::value(environment::OIDC_CA_CERTIFICATE_FILE)
                    .map(PathBuf::from),
            },
            mcp: if environment::mcp_enabled() {
                Some(McpConfig {
                    allowed_origins: validate_mcp_allowed_origins(environment::comma_separated(
                        environment::MCP_ALLOWED_ORIGINS,
                    ))?,
                    authorization: mcp_authorization()?,
                })
            } else {
                None
            },
        };
        let secrets = SecretConfig {
            oidc_client_secret: required_secret(environment::OIDC_CLIENT_SECRET)?,
        };
        Ok((configuration, secrets))
    }
}

fn mcp_authorization() -> Result<McpAuthorizationConfig, ConfigurationError> {
    Ok(McpAuthorizationConfig {
        issuer: required(environment::MCP_AUTHORIZATION_ISSUER)?,
        upstream_issuer_claim: required(environment::MCP_UPSTREAM_ISSUER_CLAIM)?,
        upstream_subject_claim: required(environment::MCP_UPSTREAM_SUBJECT_CLAIM)?,
        groups_claim: required(environment::MCP_GROUPS_CLAIM)?,
    })
}

impl StorageConfig {
    pub fn from_environment() -> Result<Self, ConfigurationError> {
        Ok(Self {
            database_url: required(environment::DATABASE_URL)?,
        })
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
    if let Some(path) = std::env::var_os(file_variable) {
        let value = std::fs::read_to_string(path)
            .map_err(|_| ConfigurationError::UnreadableSecretFile(name))?
            .trim_end_matches(['\r', '\n'])
            .to_owned();
        return (!value.is_empty())
            .then_some(value)
            .ok_or(ConfigurationError::MissingEnvironment(name))
            .map(Some);
    }
    Ok(environment::value(name))
}

fn required(name: &'static str) -> Result<String, ConfigurationError> {
    environment::value(name).ok_or(ConfigurationError::MissingEnvironment(name))
}

/// 外部から到達するURLとして受理できる形式かを検査する。
///
/// base URLとOIDC issuer URLは同じ条件で検査する。base URLはサブパスを含められるため、
/// pathは制限しない。
fn validate_https_url(name: &'static str) -> Result<Url, ConfigurationError> {
    parse_https_url(name, &required(name)?)
}

fn parse_https_url(name: &'static str, value: &str) -> Result<Url, ConfigurationError> {
    let url = Url::parse(value).map_err(|_| ConfigurationError::InvalidHttpsUrl(name))?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(ConfigurationError::InvalidHttpsUrl(name));
    }
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn https_url_rejects_non_https() {
        assert_eq!(
            parse_https_url(environment::BASE_URL, "http://example.test"),
            Err(ConfigurationError::InvalidHttpsUrl(environment::BASE_URL))
        );
    }

    #[test]
    fn base_url_accepts_subpath() {
        assert_eq!(
            parse_https_url(environment::BASE_URL, "https://example.test/marginalis")
                .expect("valid URL")
                .path(),
            "/marginalis"
        );
    }

    /// 失敗した変数名を利用者へ示せることを確認する。
    #[test]
    fn https_url_rejects_userinfo_and_names_the_variable() {
        for invalid in [
            "https://user@id.example.test",
            "https://user:password@id.example.test",
        ] {
            assert_eq!(
                parse_https_url(environment::OIDC_ISSUER_URL, invalid),
                Err(ConfigurationError::InvalidHttpsUrl(
                    environment::OIDC_ISSUER_URL
                ))
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
