//! MCPの初期化とHTTP・JSON-RPC境界の検査。

use axum::http::{HeaderMap, header};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::Deserialize;

use super::jsonrpc::{
    JsonRpcRequest, MCP_PROTOCOL_VERSION, MODERN_MCP_PROTOCOL_VERSION,
    SUPPORTED_MCP_PROTOCOL_VERSIONS,
};

const PROTOCOL_VERSION_META: &str = "io.modelcontextprotocol/protocolVersion";
const CLIENT_CAPABILITIES_META: &str = "io.modelcontextprotocol/clientCapabilities";
const CLIENT_INFO_META: &str = "io.modelcontextprotocol/clientInfo";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ProtocolEra {
    Legacy,
    Modern,
}

pub(super) enum ProtocolValidationError {
    HeaderMismatch(&'static str),
    UnsupportedVersion(String),
    InvalidMetadata,
}

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
        "serverInfo":{"name":"marginalis","version":env!("CARGO_PKG_VERSION")},
        "instructions": marginalis_contract::MCP_SERVER_INSTRUCTIONS
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

pub(super) fn validate_protocol(
    headers: &HeaderMap,
    request: &JsonRpcRequest,
) -> Result<ProtocolEra, ProtocolValidationError> {
    let header_version = header_value(headers, "mcp-protocol-version");
    let metadata = request
        .params
        .as_ref()
        .and_then(serde_json::Value::as_object)
        .and_then(|params| params.get("_meta"))
        .and_then(serde_json::Value::as_object);
    let metadata_version = metadata
        .and_then(|meta| meta.get(PROTOCOL_VERSION_META))
        .and_then(serde_json::Value::as_str);

    let modern = header_version == Some(MODERN_MCP_PROTOCOL_VERSION)
        || metadata_version == Some(MODERN_MCP_PROTOCOL_VERSION);
    if modern {
        let Some(header_version) = header_version else {
            return Err(ProtocolValidationError::HeaderMismatch(
                "MCP-Protocol-Version header is missing",
            ));
        };
        let Some(metadata_version) = metadata_version else {
            return Err(ProtocolValidationError::HeaderMismatch(
                "protocol version metadata is missing",
            ));
        };
        if header_version != metadata_version {
            return Err(ProtocolValidationError::HeaderMismatch(
                "protocol version header does not match request metadata",
            ));
        }
        if header_version != MODERN_MCP_PROTOCOL_VERSION {
            return Err(ProtocolValidationError::UnsupportedVersion(
                header_version.into(),
            ));
        }
        let Some(metadata) = metadata else {
            return Err(ProtocolValidationError::InvalidMetadata);
        };
        if !metadata
            .get(CLIENT_CAPABILITIES_META)
            .is_some_and(serde_json::Value::is_object)
            || metadata.get(CLIENT_INFO_META).is_some_and(
                |client_info| match serde_json::from_value::<McpClientInfo>(client_info.clone()) {
                    Ok(info) => info.name.trim().is_empty() || info.version.trim().is_empty(),
                    Err(_) => true,
                },
            )
        {
            return Err(ProtocolValidationError::InvalidMetadata);
        }
        validate_modern_headers(headers, request)?;
        return Ok(ProtocolEra::Modern);
    }

    if let Some(version) = header_version
        && !matches!(version, MCP_PROTOCOL_VERSION | "2025-03-26")
    {
        return Err(ProtocolValidationError::UnsupportedVersion(version.into()));
    }
    Ok(ProtocolEra::Legacy)
}

fn validate_modern_headers(
    headers: &HeaderMap,
    request: &JsonRpcRequest,
) -> Result<(), ProtocolValidationError> {
    if header_value(headers, "mcp-method") != Some(request.method.as_str()) {
        return Err(ProtocolValidationError::HeaderMismatch(
            "Mcp-Method header does not match request method",
        ));
    }
    if request.method == "tools/call" {
        let body_name = request
            .params
            .as_ref()
            .and_then(serde_json::Value::as_object)
            .and_then(|params| params.get("name"))
            .and_then(serde_json::Value::as_str)
            .ok_or(ProtocolValidationError::InvalidMetadata)?;
        let header_name = header_value(headers, "mcp-name")
            .and_then(decode_mcp_name)
            .ok_or(ProtocolValidationError::HeaderMismatch(
                "Mcp-Name header is missing or malformed",
            ))?;
        if header_name != body_name {
            return Err(ProtocolValidationError::HeaderMismatch(
                "Mcp-Name header does not match request name",
            ));
        }
    }
    Ok(())
}

fn header_value<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name).and_then(|value| value.to_str().ok())
}

pub(super) fn decode_mcp_name(value: &str) -> Option<String> {
    let Some(encoded) = value
        .strip_prefix("=?base64?")
        .and_then(|value| value.strip_suffix("?="))
    else {
        return Some(value.into());
    };
    let bytes = STANDARD.decode(encoded).ok()?;
    String::from_utf8(bytes).ok()
}

pub(super) fn supported_versions() -> serde_json::Value {
    serde_json::json!(SUPPORTED_MCP_PROTOCOL_VERSIONS)
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
