//! HTTP adapterの共有状態とMCP endpoint設定。

use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use marginalis_application::{
    McpOAuthUseCases, NoteUseCases, OidcAuthenticationUseCases, WebSessionUseCases,
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
    attempts: Arc<Mutex<VecDeque<Instant>>>,
    limit: usize,
    window: Duration,
}

impl McpRegistrationRateLimiter {
    pub(super) fn new(limit: usize, window: Duration) -> Self {
        Self {
            attempts: Arc::new(Mutex::new(VecDeque::new())),
            limit,
            window,
        }
    }

    pub(super) fn allow(&self, now: Instant) -> bool {
        let Ok(mut attempts) = self.attempts.lock() else {
            return false;
        };
        let cutoff = now.checked_sub(self.window).unwrap_or(now);
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
    pub oauth: Arc<dyn McpOAuthUseCases>,
    pub notes: Arc<dyn NoteUseCases>,
    /// Browser-based MCP clients are restricted to these exact Origins. Native clients omit
    /// `Origin` and authenticate every request with a Bearer token.
    pub allowed_origins: Vec<String>,
    pub resource_uri: String,
    pub metadata_uri: String,
    pub authorization_server_uri: String,
    pub authorization_server_metadata_uri: String,
    pub authorization_endpoint_uri: String,
    pub token_endpoint_uri: String,
}

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
