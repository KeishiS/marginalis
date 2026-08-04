//! 認証済みReactアプリケーションのHTML枠。

use marginalis_contract::ApplicationConfigResponse;

use axum::{
    extract::State,
    http::{HeaderMap, Uri},
    response::{Html, IntoResponse, Response},
};

use super::{
    auth::{authenticated_ui_actor, external_path},
    error::HandlerResult,
    html::{escape_html, page_document},
    security::ContentSecurityPolicyNonce,
    state::ApiState,
};

pub(super) async fn home(
    State(state): State<ApiState>,
    uri: Uri,
    headers: HeaderMap,
) -> HandlerResult<Response> {
    application_shell(&state, &headers, &uri).await
}

pub(super) async fn bibliography_page(
    State(state): State<ApiState>,
    uri: Uri,
    headers: HeaderMap,
) -> HandlerResult<Response> {
    application_shell(&state, &headers, &uri).await
}

pub(super) async fn graph_page(
    State(state): State<ApiState>,
    uri: Uri,
    headers: HeaderMap,
) -> HandlerResult<Response> {
    application_shell(&state, &headers, &uri).await
}

pub(super) async fn math_macro_settings_page(
    State(state): State<ApiState>,
    uri: Uri,
    headers: HeaderMap,
) -> HandlerResult<Response> {
    application_shell(&state, &headers, &uri).await
}

pub(super) async fn deleted_notes_page(
    State(state): State<ApiState>,
    uri: Uri,
    headers: HeaderMap,
) -> HandlerResult<Response> {
    application_shell(&state, &headers, &uri).await
}

pub(super) async fn view_note(
    State(state): State<ApiState>,
    uri: Uri,
    headers: HeaderMap,
) -> HandlerResult<Response> {
    application_shell(&state, &headers, &uri).await
}

pub(super) async fn access_note_page(
    State(state): State<ApiState>,
    uri: Uri,
    headers: HeaderMap,
) -> HandlerResult<Response> {
    application_shell(&state, &headers, &uri).await
}

pub(super) async fn create_note_page(
    State(state): State<ApiState>,
    uri: Uri,
    headers: HeaderMap,
) -> HandlerResult<Response> {
    application_shell(&state, &headers, &uri).await
}

pub(super) async fn edit_note_page(
    State(state): State<ApiState>,
    uri: Uri,
    headers: HeaderMap,
) -> HandlerResult<Response> {
    application_shell(&state, &headers, &uri).await
}

async fn application_shell(
    state: &ApiState,
    headers: &HeaderMap,
    uri: &Uri,
) -> HandlerResult<Response> {
    let internal_path = internal_path(&state.cookie_path, uri.path());
    let return_to = external_path(&state.cookie_path, &internal_path);
    if let Err(response) = authenticated_ui_actor(headers, state, &return_to).await {
        return Ok(response);
    }
    let style_nonce = ContentSecurityPolicyNonce::generate();
    // 埋め込む設定も公開契約の型から組み立て、Web UI側の検査と同じ形を保つ。
    let config = serde_json::to_string(&ApplicationConfigResponse {
        api_base: external_path(&state.cookie_path, "/api/v3"),
        base_path: state.cookie_path.clone(),
        path: internal_path,
        search: uri
            .query()
            .map_or(String::new(), |query| format!("?{query}")),
        style_nonce: style_nonce.as_str().to_owned(),
    })
    .expect("application configuration is serializable");
    let content = format!(
        "<div data-application-root data-application-config=\"{}\"><p>画面を読み込んでいます。</p></div><noscript>Marginalisの利用にはJavaScriptが必要です。</noscript>",
        escape_html(&config),
    );
    let mut response = Html(page_document(
        "Marginalis",
        &state.cookie_path,
        &content,
        &["/assets/page.js", "/assets/editor.js"],
    ))
    .into_response();
    response.extensions_mut().insert(style_nonce);
    Ok(response)
}

fn internal_path(cookie_path: &str, request_path: &str) -> String {
    if cookie_path == "/" {
        return request_path.to_owned();
    }
    request_path
        .strip_prefix(cookie_path.trim_end_matches('/'))
        .filter(|path| path.starts_with('/'))
        .unwrap_or(request_path)
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::internal_path;

    #[test]
    fn strips_the_public_base_path_from_application_routes() {
        assert_eq!(
            internal_path("/marginalis", "/marginalis/notes/example/edit"),
            "/notes/example/edit"
        );
        assert_eq!(internal_path("/", "/notes/example"), "/notes/example");
        assert_eq!(
            internal_path("/marginalis", "/notes/example"),
            "/notes/example"
        );
    }
}
