//! HTTP adapterの共有状態とMCP endpoint設定。

use std::sync::Arc;

use marginalis_application::{
    BibliographyUseCases, McpAccessTokenAuthenticator, NoteUseCases, OidcAuthenticationUseCases,
    WebSessionUseCases,
};

#[derive(Clone)]
pub struct ApiState {
    pub notes: Arc<dyn NoteUseCases>,
    pub bibliography: Option<Arc<dyn BibliographyUseCases>>,
    pub sessions: Arc<dyn WebSessionUseCases>,
    pub oidc: Arc<dyn OidcAuthenticationUseCases>,
    pub cookie_path: String,
    pub browser_origin: String,
    pub mcp: Option<Arc<McpEndpoint>>,
}

pub struct McpEndpoint {
    pub(super) access_token_authenticator: Arc<dyn McpAccessTokenAuthenticator>,
    /// MCP requests that carry `Origin` are restricted to these exact values. Backend and native
    /// clients normally omit `Origin` and authenticate every request with a Bearer token.
    pub(super) allowed_origins: Vec<String>,
    pub(super) resource_uri: String,
    pub(super) metadata_uri: String,
    pub(super) authorization_server_uri: String,
}

impl McpEndpoint {
    pub fn new(
        base_url: &url::Url,
        allowed_origins: Vec<String>,
        authorization_server_uri: String,
        access_token_authenticator: Arc<dyn McpAccessTokenAuthenticator>,
    ) -> Result<Self, McpAuthorizationServerConfigurationError> {
        let issuer = url::Url::parse(&authorization_server_uri)
            .map_err(|_| McpAuthorizationServerConfigurationError)?;
        if issuer.scheme() != "https"
            || issuer.host_str().is_none()
            || !issuer.username().is_empty()
            || issuer.password().is_some()
            || issuer.query().is_some()
            || issuer.fragment().is_some()
        {
            return Err(McpAuthorizationServerConfigurationError);
        }
        let resource_uri = base_url_at(base_url, "mcp");
        Ok(Self {
            access_token_authenticator,
            allowed_origins,
            metadata_uri: well_known_url(&resource_uri, "oauth-protected-resource").to_string(),
            authorization_server_uri,
            resource_uri: resource_uri.to_string(),
        })
    }

    pub fn resource_uri_for(base_url: &url::Url) -> String {
        base_url_at(base_url, "mcp").to_string()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("MCP Authorization Server URL is invalid")]
pub struct McpAuthorizationServerConfigurationError;

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
            bibliography: None,
            sessions,
            oidc,
            cookie_path,
            browser_origin,
            mcp: None,
        }
    }

    pub fn with_mcp(mut self, mcp: McpEndpoint) -> Self {
        self.mcp = Some(Arc::new(mcp));
        self
    }

    pub fn with_bibliography(mut self, bibliography: Arc<dyn BibliographyUseCases>) -> Self {
        self.bibliography = Some(bibliography);
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
