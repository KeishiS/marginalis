//! Marginalisが提供するMCP OAuth authorization server境界。

mod authorization;
mod common;
mod registration;
mod revocation;
mod token;

pub(super) use authorization::{
    mcp_authorize, mcp_authorize_consent, mcp_authorize_post, mcp_resource_metadata,
    mcp_server_metadata,
};
pub(super) use registration::mcp_register_client;
pub(super) use revocation::{mcp_revoke_token, revoke_mcp_authorization};
pub(super) use token::mcp_token;
