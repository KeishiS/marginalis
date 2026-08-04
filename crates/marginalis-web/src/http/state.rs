//! HTTP adapterの共有状態とMCP endpoint設定。

use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use marginalis_application::{
    BibliographyUseCases, MathMacroUseCases, McpOAuthUseCases, NoteUseCases,
    OidcAuthenticationUseCases, WebSessionUseCases,
};
use mcp_authorization_server::{AuthorizationServerEndpoints, ResourcePolicy};

#[derive(Clone)]
pub struct ApiState {
    pub notes: Arc<dyn NoteUseCases>,
    pub bibliography: Option<Arc<dyn BibliographyUseCases>>,
    pub math_macros: Arc<dyn MathMacroUseCases>,
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
        if attempts.len() >= self.limit {
            return false;
        }
        attempts.push_back(now);
        true
    }
}

pub struct McpEndpoint {
    pub(super) oauth: Arc<dyn McpOAuthUseCases>,
    /// MCP requests that carry `Origin` are restricted to these exact values. Backend and native
    /// clients normally omit `Origin` and authenticate every request with a Bearer token.
    pub(super) allowed_origins: Vec<String>,
    pub(super) resource_policy: ResourcePolicy,
    pub(super) metadata_uri: String,
    pub(super) authorization_server_uri: String,
    pub(super) authorization_server_metadata_uri: String,
    pub(super) authorization_server_endpoints: AuthorizationServerEndpoints,
}

impl McpEndpoint {
    pub fn new(
        oauth: Arc<dyn McpOAuthUseCases>,
        resource_policy: ResourcePolicy,
        base_url: &url::Url,
        allowed_origins: Vec<String>,
    ) -> Self {
        let resource_uri = resource_policy.uri().clone();
        let authorization_server_uri = base_url.to_string();
        Self {
            oauth,
            allowed_origins,
            metadata_uri: well_known_url(&resource_uri, "oauth-protected-resource").to_string(),
            authorization_server_uri: authorization_server_uri.clone(),
            authorization_server_metadata_uri: well_known_url(
                base_url,
                "oauth-authorization-server",
            )
            .to_string(),
            authorization_server_endpoints: AuthorizationServerEndpoints {
                issuer: authorization_server_uri,
                authorization_endpoint: base_url_at(base_url, "oauth/authorize").to_string(),
                token_endpoint: base_url_at(base_url, "oauth/token").to_string(),
                revocation_endpoint: base_url_at(base_url, "oauth/revoke").to_string(),
                registration_endpoint: base_url_at(base_url, "oauth/register").to_string(),
            },
            resource_policy,
        }
    }

    pub fn resource_uri_for(base_url: &url::Url) -> String {
        base_url_at(base_url, "mcp").to_string()
    }
}

impl ApiState {
    pub fn new(
        notes: Arc<dyn NoteUseCases>,
        math_macros: Arc<dyn MathMacroUseCases>,
        sessions: Arc<dyn WebSessionUseCases>,
        oidc: Arc<dyn OidcAuthenticationUseCases>,
        cookie_path: String,
        browser_origin: String,
    ) -> Self {
        Self {
            notes,
            math_macros,
            bibliography: None,
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
