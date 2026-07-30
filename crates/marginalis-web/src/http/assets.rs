//! ビルド時に固定したWeb UIアセットの配信。

use axum::{
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};

const EDITOR_JAVASCRIPT: &[u8] = include_bytes!("../../../../frontend/dist/assets/editor.js");
const EDITOR_STYLESHEET: &[u8] = include_bytes!("../../../../frontend/dist/assets/editor.css");
const MATHJAX_JAVASCRIPT: &[u8] = include_bytes!("../../../../frontend/dist/assets/tex-svg.js");
const PAGE_JAVASCRIPT: &[u8] = include_bytes!("../../../../frontend/dist/assets/page.js");
include!(concat!(env!("OUT_DIR"), "/mathjax_font_assets.rs"));

pub(super) async fn editor_javascript() -> Response {
    asset("text/javascript; charset=utf-8", EDITOR_JAVASCRIPT)
}

pub(super) async fn editor_stylesheet() -> Response {
    asset("text/css; charset=utf-8", EDITOR_STYLESHEET)
}

pub(super) async fn mathjax_javascript() -> Response {
    asset("text/javascript; charset=utf-8", MATHJAX_JAVASCRIPT)
}

pub(super) async fn mathjax_font_javascript(
    axum::extract::Path(file_name): axum::extract::Path<String>,
) -> Response {
    match MATHJAX_FONT_FILES
        .iter()
        .find(|(candidate, _)| *candidate == file_name)
    {
        Some((_, body)) => asset("text/javascript; charset=utf-8", body),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

pub(super) async fn page_javascript() -> Response {
    asset("text/javascript; charset=utf-8", PAGE_JAVASCRIPT)
}

fn asset(content_type: &'static str, body: &'static [u8]) -> Response {
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, HeaderValue::from_static(content_type)),
            (
                header::CONTENT_LENGTH,
                HeaderValue::from_str(&body.len().to_string()).expect("asset length is valid"),
            ),
        ],
        body,
    )
        .into_response()
}
