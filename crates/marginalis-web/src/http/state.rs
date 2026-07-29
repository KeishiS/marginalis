//! HTTP adapterの共有状態とMCP endpoint設定。

use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use marginalis_application::{
    McpAccessTokenAuthenticator, McpOAuthUseCases, NoteUseCases, OidcAuthenticationUseCases,
    WebSessionUseCases,
};

#[derive(Clone)]
pub struct ApiState {
    pub notes: Arc<dyn NoteUseCases>,
    pub sessions: Arc<dyn WebSessionUseCases>,
    pub oidc: Arc<dyn OidcAuthenticationUseCases>,
    pub cookie_path: String,
    pub browser_origin: String,
    pub mcp: Option<Arc<McpEndpoint>>,
    pub(super) mcp_registration_limiter: McpRegistrationRateLimiter,
}

#[derive(Clone)]
pub(super) struct McpRegistrationRateLimiter {
    attempts_by_redirect_origin: Arc<Mutex<HashMap<String, VecDeque<Instant>>>>,
    limit: usize,
    window: Duration,
}

impl McpRegistrationRateLimiter {
    pub(super) fn new(limit: usize, window: Duration) -> Self {
        Self {
            attempts_by_redirect_origin: Arc::new(Mutex::new(HashMap::new())),
            limit,
            window,
        }
    }

    pub(super) fn allow(&self, redirect_origin: &str, now: Instant) -> bool {
        let Ok(mut attempts_by_origin) = self.attempts_by_redirect_origin.lock() else {
            return false;
        };
        let cutoff = now.checked_sub(self.window).unwrap_or(now);
        for attempts in attempts_by_origin.values_mut() {
            while attempts.front().is_some_and(|attempt| *attempt <= cutoff) {
                attempts.pop_front();
            }
        }
        attempts_by_origin.retain(|_, attempts| !attempts.is_empty());
        if !attempts_by_origin.contains_key(redirect_origin) && attempts_by_origin.len() >= 1_024 {
            return false;
        }
        let attempts = attempts_by_origin
            .entry(redirect_origin.to_owned())
            .or_default();
        while attempts.front().is_some_and(|attempt| *attempt <= cutoff) {
            attempts.pop_front();
        }
        if attempts.len() >= self.limit {
            return false;
        }
        attempts.push_back(now);
        true
    }
}

pub struct McpEndpoint {
    pub(super) oauth: Arc<dyn McpOAuthUseCases>,
    pub(super) external_access_token_authenticator: Option<Arc<dyn McpAccessTokenAuthenticator>>,
    /// MCP requests that carry `Origin` are restricted to these exact values. Backend and native
    /// clients normally omit `Origin` and authenticate every request with a Bearer token.
    pub(super) allowed_origins: Vec<String>,
    pub(super) resource_uri: String,
    pub(super) metadata_uri: String,
    pub(super) authorization_server_uri: String,
    pub(super) authorization_server_metadata_uri: String,
    pub(super) authorization_endpoint_uri: String,
    pub(super) token_endpoint_uri: String,
}

impl McpEndpoint {
    pub fn new(
        oauth: Arc<dyn McpOAuthUseCases>,
        base_url: &url::Url,
        allowed_origins: Vec<String>,
    ) -> Self {
        let resource_uri = base_url_at(base_url, "mcp");
        Self {
            oauth,
            external_access_token_authenticator: None,
            allowed_origins,
            metadata_uri: well_known_url(&resource_uri, "oauth-protected-resource").to_string(),
            authorization_server_uri: base_url.to_string(),
            authorization_server_metadata_uri: well_known_url(
                base_url,
                "oauth-authorization-server",
            )
            .to_string(),
            authorization_endpoint_uri: base_url_at(base_url, "oauth/authorize").to_string(),
            token_endpoint_uri: base_url_at(base_url, "oauth/token").to_string(),
            resource_uri: resource_uri.to_string(),
        }
    }

    pub fn resource_uri_for(base_url: &url::Url) -> String {
        base_url_at(base_url, "mcp").to_string()
    }

    pub fn with_external_authorization_server(
        mut self,
        authorization_server_uri: String,
        authenticator: Arc<dyn McpAccessTokenAuthenticator>,
    ) -> Result<Self, ExternalAuthorizationServerConfigurationError> {
        let issuer = url::Url::parse(&authorization_server_uri)
            .map_err(|_| ExternalAuthorizationServerConfigurationError)?;
        if issuer.scheme() != "https"
            || issuer.host_str().is_none()
            || !issuer.username().is_empty()
            || issuer.password().is_some()
            || issuer.query().is_some()
            || issuer.fragment().is_some()
        {
            return Err(ExternalAuthorizationServerConfigurationError);
        }
        self.authorization_server_uri = authorization_server_uri;
        self.external_access_token_authenticator = Some(authenticator);
        Ok(self)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExternalAuthorizationServerConfigurationError;

impl core::fmt::Display for ExternalAuthorizationServerConfigurationError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str("external authorization server URL is invalid")
    }
}

impl std::error::Error for ExternalAuthorizationServerConfigurationError {}

impl ApiState {
    pub fn new(
        notes: Arc<dyn NoteUseCases>,
        sessions: Arc<dyn WebSessionUseCases>,
        oidc: Arc<dyn OidcAuthenticationUseCases>,
        cookie_path: String,
        browser_origin: String,
    ) -> Self {
        Self {
            notes,
            sessions,
            oidc,
            cookie_path,
            browser_origin,
            mcp: None,
            mcp_registration_limiter: McpRegistrationRateLimiter::new(
                30,
                Duration::from_secs(10 * 60),
            ),
        }
    }

    pub fn with_mcp(mut self, mcp: McpEndpoint) -> Self {
        self.mcp = Some(Arc::new(mcp));
        self
    }
}

fn base_url_at(base_url: &url::Url, suffix: &str) -> url::Url {
    let mut url = base_url.clone();
    let prefix = base_url.path().trim_matches('/');
    url.set_path(
        if prefix.is_empty() {
            format!("/{suffix}")
        } else {
            format!("/{prefix}/{suffix}")
        }
        .as_str(),
    );
    url
}

/// RFC 8414/9728に従い、subject URLのhostとpathの間へwell-known suffixを挿入する。
fn well_known_url(subject: &url::Url, suffix: &str) -> url::Url {
    let mut url = subject.clone();
    let subject_path = subject.path().trim_end_matches('/');
    url.set_path(
        if subject_path.is_empty() {
            format!("/.well-known/{suffix}")
        } else {
            format!("/.well-known/{suffix}{subject_path}")
        }
        .as_str(),
    );
    url
}

#[cfg(test)]
mod tests {
    use super::well_known_url;

    #[test]
    fn well_known_urls_insert_the_suffix_before_a_subject_path() {
        let root = url::Url::parse("https://example.test").expect("root URL");
        assert_eq!(
            well_known_url(&root, "oauth-authorization-server").as_str(),
            "https://example.test/.well-known/oauth-authorization-server"
        );

        let issuer = url::Url::parse("https://example.test/marginalis").expect("issuer URL");
        assert_eq!(
            well_known_url(&issuer, "oauth-authorization-server").as_str(),
            "https://example.test/.well-known/oauth-authorization-server/marginalis"
        );

        let resource =
            url::Url::parse("https://example.test/marginalis/mcp").expect("resource URL");
        assert_eq!(
            well_known_url(&resource, "oauth-protected-resource").as_str(),
            "https://example.test/.well-known/oauth-protected-resource/marginalis/mcp"
        );
    }
}
