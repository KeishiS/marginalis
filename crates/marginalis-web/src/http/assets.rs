//! ビルド時に固定したWeb UIアセットの配信。

use axum::{
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};

include!(concat!(env!("OUT_DIR"), "/mathjax_font_assets.rs"));
include!(concat!(env!("OUT_DIR"), "/web_font_assets.rs"));
include!(concat!(env!("OUT_DIR"), "/bundle_assets.rs"));

/// `dist/assets`直下の配布物を名前で引いて返す。
///
/// 名前を経路へ書き並べません。分割読み込みで増えるchunkは名前へhashが付くため、書き並べる方式では
/// 新しい出力が配信されないまま気付けません。実際に、関係の図のchunkが404になり、moduleとして
/// 読み込めずに画面全体が空になりました。
pub(super) async fn bundle_asset(
    axum::extract::Path(file_name): axum::extract::Path<String>,
) -> Response {
    match BUNDLE_FILES
        .iter()
        .find(|(candidate, _, _)| *candidate == file_name)
    {
        Some((_, content_type, body)) => asset(content_type, body),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

/// 配信している`dist/assets`直下のファイル名。配布物と経路の対応を試験で確かめるために公開します。
#[cfg(test)]
pub(crate) fn bundled_asset_names() -> impl Iterator<Item = &'static str> {
    BUNDLE_FILES.iter().map(|(name, _, _)| *name)
}

pub(super) async fn mathjax_font_javascript(
    axum::extract::Path(file_name): axum::extract::Path<String>,
) -> Response {
    named_asset(
        MATHJAX_FONT_FILES,
        "text/javascript; charset=utf-8",
        &file_name,
    )
}

pub(super) async fn web_font(
    axum::extract::Path(file_name): axum::extract::Path<String>,
) -> Response {
    named_asset(WEB_FONT_FILES, "font/woff2", &file_name)
}

fn named_asset(
    files: &'static [(&'static str, &'static [u8])],
    content_type: &'static str,
    file_name: &str,
) -> Response {
    match files.iter().find(|(candidate, _)| *candidate == file_name) {
        Some((_, body)) => asset(content_type, body),
        None => StatusCode::NOT_FOUND.into_response(),
    }
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
