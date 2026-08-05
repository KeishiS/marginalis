//! REST、MCP、TypeScriptで共有する公開契約の正本。

#![recursion_limit = "256"]

mod mcp;
mod rest;

pub use mcp::*;
pub use rest::*;
