//! Marginalisのapplication portをproduction adapterへ接続するservice層。

mod config;
mod runtime;

pub use config::{
    ConfigurationError, HttpConfig, OidcConfig, SecretConfig, ServerConfig, StorageConfig,
};
pub use runtime::{SystemClock, SystemRandom};
