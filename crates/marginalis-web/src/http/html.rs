//! サーバー生成HTMLの共通要素。

use super::auth::external_path;

pub(super) fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

pub(super) fn page_document(
    title: &str,
    cookie_path: &str,
    content: &str,
    scripts: &[&str],
) -> String {
    let home = external_path(cookie_path, "/");
    let new_note = external_path(cookie_path, "/notes/new");
    let bibliography = external_path(cookie_path, "/bibliography");
    let graph = external_path(cookie_path, "/graph");
    let settings = external_path(cookie_path, "/settings");
    let stylesheet = external_path(cookie_path, "/assets/editor.css");
    let scripts = scripts
        .iter()
        .map(|script| {
            format!(
                "<script src=\"{}\" type=\"module\"></script>",
                escape_html(&external_path(cookie_path, script))
            )
        })
        .collect::<String>();
    format!(
        "<!doctype html><html lang=\"ja\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>{}</title><link rel=\"stylesheet\" href=\"{}\">{}</head><body><header class=\"page-header\"><div class=\"page-header-inner\"><a class=\"brand\" href=\"{}\"><span class=\"brand-mark\" aria-hidden=\"true\">M</span><span>Marginalis</span></a><nav class=\"primary-navigation\" aria-label=\"主要な画面\"><a href=\"{}\">ノート</a><a href=\"{}\">書誌</a><a href=\"{}\">関係の図</a><a href=\"{}\">設定</a><a class=\"button button-primary\" href=\"{}\">新規ノート</a></nav></div></header><main class=\"page-main\">{}</main></body></html>",
        escape_html(title),
        escape_html(&stylesheet),
        scripts,
        escape_html(&home),
        escape_html(&home),
        escape_html(&bibliography),
        escape_html(&graph),
        escape_html(&settings),
        escape_html(&new_note),
        content,
    )
}

#[cfg(test)]
mod tests {
    use super::{escape_html, page_document};

    #[test]
    fn escapes_text_for_html_contexts() {
        assert_eq!(
            escape_html("日本語 & <tag> \"double\" 'single'"),
            "日本語 &amp; &lt;tag&gt; &quot;double&quot; &#39;single&#39;"
        );
    }

    #[test]
    fn page_document_uses_japanese_language_and_subpath_assets() {
        let document = page_document("<題名>", "/marginalis", "<h1>本文</h1>", &[]);

        assert!(document.contains("<html lang=\"ja\">"));
        assert!(document.contains("<title>&lt;題名&gt;</title>"));
        assert!(document.contains("href=\"/marginalis/assets/editor.css\""));
        assert!(!document.contains("<script"));
        assert!(document.contains("href=\"/marginalis/\""));
        assert!(document.contains("aria-label=\"主要な画面\""));
        assert!(document.contains("href=\"/marginalis/notes/new\""));
        assert!(document.contains("<main class=\"page-main\"><h1>本文</h1></main>"));
        assert!(!document.contains("src=\"/marginalis/assets/editor.js\""));
    }

    #[test]
    fn page_document_can_load_a_subpath_module() {
        let document = page_document(
            "編集",
            "/marginalis",
            "<div></div>",
            &["/assets/page.js", "/assets/editor.js"],
        );

        assert!(document.contains("src=\"/marginalis/assets/page.js\""));
        assert!(
            document
                .contains("<script src=\"/marginalis/assets/editor.js\" type=\"module\"></script>")
        );
    }
}
