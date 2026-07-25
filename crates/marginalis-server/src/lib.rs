//! Marginalisのapplication portをproduction adapterへ接続するservice層。

mod config;
mod mcp_oauth;
mod notes;
mod oidc;
mod runtime;
mod session;

pub use config::{
    ConfigurationError, HttpConfig, OidcConfig, SecretConfig, ServerConfig, StorageConfig,
};
pub use mcp_oauth::{McpIssuedTokenPair, McpOAuthError, ServerMcpOAuthService};
pub use notes::ServerNoteUseCases;
pub use oidc::ServerOidcAuthenticationUseCases;
pub use runtime::{SystemClock, SystemRandom};
pub use session::ServerWebSessionUseCases;
