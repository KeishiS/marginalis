//! OIDC provider接続の設定境界。HTTP handlerやSQLiteへは依存しない。

use core::fmt;

use openidconnect::{ClientId, ClientSecret, IssuerUrl, RedirectUrl};
use url::Url;

#[derive(Clone)]
pub struct OidcConfiguration {
    issuer_url: IssuerUrl,
    client_id: ClientId,
    client_secret: ClientSecret,
    redirect_url: RedirectUrl,
    cookie_path: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OidcConfigurationError {
    InvalidIssuerUrl,
    InvalidBaseUrl,
}

impl fmt::Display for OidcConfigurationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidIssuerUrl => formatter.write_str("OIDC issuer URL is invalid"),
            Self::InvalidBaseUrl => formatter.write_str("Base URL must be an absolute HTTPS URL"),
        }
    }
}

impl std::error::Error for OidcConfigurationError {}

impl OidcConfiguration {
    pub fn new(
        issuer_url: String,
        client_id: String,
        client_secret: String,
        base_url: &str,
    ) -> Result<Self, OidcConfigurationError> {
        let issuer_url =
            IssuerUrl::new(issuer_url).map_err(|_| OidcConfigurationError::InvalidIssuerUrl)?;
        let redirect_url = callback_url(base_url)?;
        let cookie_path = cookie_path(base_url)?;
        Ok(Self {
            issuer_url,
            client_id: ClientId::new(client_id),
            client_secret: ClientSecret::new(client_secret),
            redirect_url,
            cookie_path,
        })
    }

    pub fn issuer_url(&self) -> &IssuerUrl {
        &self.issuer_url
    }
    pub fn client_id(&self) -> &ClientId {
        &self.client_id
    }
    pub fn client_secret(&self) -> &ClientSecret {
        &self.client_secret
    }
    pub fn redirect_url(&self) -> &RedirectUrl {
        &self.redirect_url
    }
    pub fn cookie_path(&self) -> &str {
        &self.cookie_path
    }
}

fn callback_url(base_url: &str) -> Result<RedirectUrl, OidcConfigurationError> {
    let mut url = Url::parse(base_url).map_err(|_| OidcConfigurationError::InvalidBaseUrl)?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(OidcConfigurationError::InvalidBaseUrl);
    }
    let base_path = url.path().trim_end_matches('/');
    url.set_path(&format!("{base_path}/auth/oidc/callback"));
    RedirectUrl::new(url.into()).map_err(|_| OidcConfigurationError::InvalidBaseUrl)
}

fn cookie_path(base_url: &str) -> Result<String, OidcConfigurationError> {
    let url = Url::parse(base_url).map_err(|_| OidcConfigurationError::InvalidBaseUrl)?;
    let path = url.path().trim_end_matches('/');
    Ok(if path.is_empty() {
        "/".into()
    } else {
        path.into()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preserves_base_subpath() {
        let config = OidcConfiguration::new(
            "https://id.example.test".into(),
            "client".into(),
            "secret".into(),
            "https://example.test/app/",
        )
        .expect("config");
        assert_eq!(
            config.redirect_url().as_str(),
            "https://example.test/app/auth/oidc/callback"
        );
        assert_eq!(config.cookie_path(), "/app");
    }
}
