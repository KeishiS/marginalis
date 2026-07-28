//! Marginalisのapplication portをproduction adapterへ接続するservice層。

mod config;
mod mcp_oauth;
mod oidc;
mod runtime;

pub use config::{
    ConfigurationError, HttpConfig, OidcConfig, SecretConfig, ServerConfig, StorageConfig,
};
pub use mcp_oauth::ServerMcpOAuthService;
pub use oidc::ServerOidcAuthenticationUseCases;
pub use runtime::{SystemClock, SystemRandom};
