//! MCPの初期化とHTTP・JSON-RPC境界の検査。

use axum::http::{HeaderMap, header};
use serde::Deserialize;

use crate::mcp::MCP_PROTOCOL_VERSION;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct McpInitialize {
    protocol_version: String,
    capabilities: serde_json::Value,
    client_info: McpClientInfo,
}

#[derive(Deserialize)]
struct McpClientInfo {
    name: String,
    version: String,
}

pub(super) fn initialize_result(
    params: Option<serde_json::Value>,
) -> Result<serde_json::Value, ()> {
    let initialization =
        serde_json::from_value::<McpInitialize>(params.unwrap_or_default()).map_err(|_| ())?;
    if !valid_client_capabilities(&initialization.capabilities)
        || initialization.client_info.name.trim().is_empty()
        || initialization.client_info.version.trim().is_empty()
    {
        return Err(());
    }
    let protocol_version = if matches!(
        initialization.protocol_version.as_str(),
        MCP_PROTOCOL_VERSION | "2025-03-26"
    ) {
        initialization.protocol_version
    } else {
        MCP_PROTOCOL_VERSION.into()
    };
    Ok(serde_json::json!({
        "protocolVersion": protocol_version,
        "capabilities":{"tools":{}},
        "serverInfo":{"name":"marginalis","version":env!("CARGO_PKG_VERSION")}
    }))
}

fn valid_client_capabilities(capabilities: &serde_json::Value) -> bool {
    let Some(capabilities) = capabilities.as_object() else {
        return false;
    };
    for name in ["experimental", "sampling", "elicitation"] {
        if capabilities
            .get(name)
            .is_some_and(|value| !value.is_object())
        {
            return false;
        }
    }
    capabilities.get("roots").is_none_or(|roots| {
        roots.as_object().is_some_and(|roots| {
            roots
                .get("listChanged")
                .is_none_or(serde_json::Value::is_boolean)
        })
    })
}

pub(super) fn valid_tools_list_params(params: Option<&serde_json::Value>) -> bool {
    params.is_none_or(|params| {
        params
            .as_object()
            .is_some_and(|params| !params.contains_key("cursor"))
    })
}

pub(super) fn accepts_media_type(headers: &HeaderMap, expected: &str) -> bool {
    headers.get_all(header::ACCEPT).iter().any(|value| {
        value.to_str().ok().is_some_and(|value| {
            value.split(',').any(|item| {
                let mut parts = item.split(';');
                let media_type = parts.next().unwrap_or_default().trim();
                let refused = parts.any(|parameter| {
                    parameter
                        .trim()
                        .split_once('=')
                        .is_some_and(|(name, value)| {
                            name.eq_ignore_ascii_case("q")
                                && value
                                    .trim()
                                    .parse::<f32>()
                                    .is_ok_and(|quality| quality == 0.0)
                        })
                });
                media_type.eq_ignore_ascii_case(expected) && !refused
            })
        })
    })
}

pub(super) fn content_type_is_json(headers: &HeaderMap) -> bool {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .is_some_and(|value| value.trim().eq_ignore_ascii_case("application/json"))
}

pub(super) fn protocol_version_is_supported(headers: &HeaderMap) -> bool {
    let protocol_version = headers
        .get("mcp-protocol-version")
        .map(|value| value.to_str().ok())
        .unwrap_or(Some("2025-03-26"));
    matches!(protocol_version, Some(MCP_PROTOCOL_VERSION | "2025-03-26"))
}

pub(super) fn detected_request_id(value: &serde_json::Value) -> serde_json::Value {
    value
        .as_object()
        .and_then(|object| object.get("id"))
        .filter(|id| match id {
            serde_json::Value::String(_) => true,
            serde_json::Value::Number(number) => {
                number.as_i64().is_some() || number.as_u64().is_some()
            }
            _ => false,
        })
        .cloned()
        .unwrap_or(serde_json::Value::Null)
}
