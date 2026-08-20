//! HTTP adapterの共有状態とMCP endpoint設定。

use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use marginalis_application::{
    BibliographyApplication, BibliographyImportUseCases, MathMacroUseCases, McpOAuthUseCases,
    NoteUseCases, OidcAuthenticationUseCases, WebSessionUseCases, WebhookUseCases,
};
use mcp_authorization_server::{AuthorizationServerEndpoints, ResourcePolicy};
use oidc_browser_login::cookie::SessionCookies;

#[derive(Clone)]
pub struct ApiState {
    pub(super) notes: Arc<dyn NoteUseCases>,
    pub(super) bibliography: Arc<BibliographyApplication>,
    pub(super) bibliography_import: Arc<dyn BibliographyImportUseCases>,
    pub(super) math_macros: Arc<dyn MathMacroUseCases>,
    pub(super) sessions: Arc<dyn WebSessionUseCases>,
    pub(super) oidc: Arc<dyn OidcAuthenticationUseCases>,
    pub(super) cookie_path: String,
    /// cookie pathから導出したsessionとCSRFのcookie名・属性。
    pub(super) cookies: SessionCookies,
    pub(super) browser_origin: String,
    pub(super) mcp: Option<Arc<McpEndpoint>>,
    pub(super) webhooks: Arc<dyn WebhookUseCases>,
    pub(super) mcp_registration_limiter: McpRegistrationRateLimiter,
}

/// HTTP adapterが常に利用できるapplication serviceの集合。
///
/// 本番で必ず構成する機能を`Option`にすると、起動後に一部のrouteだけが503になる状態を型で
/// 作れてしまう。任意機能であるMCP endpointだけは[`ApiState`]側で別に保持する。
pub struct ApiServices {
    pub notes: Arc<dyn NoteUseCases>,
    pub bibliography: Arc<BibliographyApplication>,
    pub bibliography_import: Arc<dyn BibliographyImportUseCases>,
    pub math_macros: Arc<dyn MathMacroUseCases>,
    pub sessions: Arc<dyn WebSessionUseCases>,
    pub oidc: Arc<dyn OidcAuthenticationUseCases>,
    pub webhooks: Arc<dyn WebhookUseCases>,
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

/// MCPと外部検索同期が共有するOAuth resource設定。
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidMcpEndpoint;

impl std::fmt::Display for InvalidMcpEndpoint {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("MCP resource policy does not match the public endpoint")
    }
}

impl std::error::Error for InvalidMcpEndpoint {}

impl McpEndpoint {
    pub fn new(
        oauth: Arc<dyn McpOAuthUseCases>,
        base_url: &url::Url,
        allowed_origins: Vec<String>,
    ) -> Result<Self, InvalidMcpEndpoint> {
        let resource_policy = oauth.resource_policy();
        if resource_policy.uri() != &base_url_at(base_url, "mcp") {
            return Err(InvalidMcpEndpoint);
        }
        let resource_uri = resource_policy.uri().clone();
        let authorization_server_uri = base_url.to_string();
        Ok(Self {
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
        })
    }

    pub fn resource_uri_for(base_url: &url::Url) -> String {
        base_url_at(base_url, "mcp").to_string()
    }
}

impl ApiState {
    pub fn new(services: ApiServices, cookie_path: String, browser_origin: String) -> Self {
        // cookie pathは検証済みbase URLから導出され、常に`/`で始まる。
        let cookies = SessionCookies::new("marginalis", &cookie_path)
            .expect("cookie path is derived from a validated base URL");
        Self {
            notes: services.notes,
            math_macros: services.math_macros,
            bibliography: services.bibliography,
            bibliography_import: services.bibliography_import,
            sessions: services.sessions,
            oidc: services.oidc,
            cookie_path,
            cookies,
            browser_origin,
            mcp: None,
            webhooks: services.webhooks,
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
