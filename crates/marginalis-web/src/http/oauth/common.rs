use std::collections::HashMap;

use axum::{
    Json,
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::Serialize;

#[derive(Default)]
pub(super) struct OAuthParameters {
    values: HashMap<String, String>,
    pub(super) repeated: bool,
}

impl OAuthParameters {
    pub(super) fn append(&mut self, encoded: &str, known: &[&str]) {
        for (name, value) in url::form_urlencoded::parse(encoded.as_bytes()) {
            if !known.contains(&name.as_ref()) || value.is_empty() {
                continue;
            }
            if self
                .values
                .insert(name.into_owned(), value.into_owned())
                .is_some()
            {
                self.repeated = true;
            }
        }
    }

    pub(super) fn get(&self, name: &str) -> Option<&str> {
        self.values.get(name).map(String::as_str)
    }

    pub(super) fn take(&mut self, name: &str) -> Option<String> {
        self.values.remove(name)
    }
}

#[derive(Serialize)]
struct OAuthErrorResponse {
    error: &'static str,
    error_description: &'static str,
}

pub(super) fn oauth_error_response(
    status: StatusCode,
    error: &'static str,
    description: &'static str,
) -> Response {
    (
        status,
        [
            (header::CACHE_CONTROL, "no-store"),
            (header::PRAGMA, "no-cache"),
        ],
        Json(OAuthErrorResponse {
            error,
            error_description: description,
        }),
    )
        .into_response()
}

pub(super) fn content_type_is(headers: &HeaderMap, expected: &str) -> bool {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .is_some_and(|value| value.trim().eq_ignore_ascii_case(expected))
}

pub(super) fn log_mcp_oauth_result<T>(operation: &'static str, result: &Result<T, Response>) {
    match result {
        Ok(_) => tracing::info!(
            event = "mcp.oauth.operation.completed",
            operation,
            "MCP OAuth operation succeeded"
        ),
        Err(response) => tracing::warn!(
            event = "mcp.oauth.operation.failed",
            operation,
            status = response.status().as_u16(),
            "MCP OAuth operation failed"
        ),
    }
}
