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

pub(super) fn page_document_with_script(
    title: &str,
    cookie_path: &str,
    content: &str,
    script: Option<&str>,
) -> String {
    let home = external_path(cookie_path, "/");
    let stylesheet = external_path(cookie_path, "/assets/editor.css");
    let page_script = external_path(cookie_path, "/assets/page.js");
    let script = script.map_or_else(String::new, |script| {
        format!(
            "<script src=\"{}\" type=\"module\"></script>",
            escape_html(&external_path(cookie_path, script))
        )
    });
    format!(
        "<!doctype html><html lang=\"ja\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>{}</title><link rel=\"stylesheet\" href=\"{}\"><script src=\"{}\" type=\"module\"></script>{}</head><body><header class=\"page-header\"><a href=\"{}\">Marginalis</a></header><main class=\"page-main\">{}</main></body></html>",
        escape_html(title),
        escape_html(&stylesheet),
        escape_html(&page_script),
        script,
        escape_html(&home),
        content
    )
}

#[cfg(test)]
mod tests {
    use super::{escape_html, page_document_with_script};

    #[test]
    fn escapes_text_for_html_contexts() {
        assert_eq!(
            escape_html("日本語 & <tag> \"double\" 'single'"),
            "日本語 &amp; &lt;tag&gt; &quot;double&quot; &#39;single&#39;"
        );
    }

    #[test]
    fn page_document_uses_japanese_language_and_subpath_assets() {
        let document = page_document_with_script("<題名>", "/marginalis", "<h1>本文</h1>", None);

        assert!(document.contains("<html lang=\"ja\">"));
        assert!(document.contains("<title>&lt;題名&gt;</title>"));
        assert!(document.contains("href=\"/marginalis/assets/editor.css\""));
        assert!(document.contains("src=\"/marginalis/assets/page.js\""));
        assert!(document.contains("href=\"/marginalis/\""));
        assert!(document.contains("<main class=\"page-main\"><h1>本文</h1></main>"));
        assert!(!document.contains("src=\"/marginalis/assets/editor.js\""));
    }

    #[test]
    fn page_document_can_load_a_subpath_module() {
        let document = page_document_with_script(
            "編集",
            "/marginalis",
            "<div></div>",
            Some("/assets/editor.js"),
        );

        assert!(
            document
                .contains("<script src=\"/marginalis/assets/editor.js\" type=\"module\"></script>")
        );
    }
}
