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

/// 主要な移動先。`current_path`と照合して、いま開いている画面を示す。
const NAVIGATION: &[(&str, &str, &str)] = &[
    ("/", "/notes", "ノート"),
    ("/bibliography", "/bibliography", "書誌"),
    ("/graph", "/graph", "関係の図"),
    ("/settings", "/settings", "設定"),
];

/// 主要な移動先のうち、`current_path`が属するものを返す。
///
/// ノート一覧は``/``だが、個別のノートは``/notes/...``にある。どちらもノートの画面として扱う。
fn current_navigation(current_path: &str) -> Option<&'static str> {
    NAVIGATION
        .iter()
        .find(|(href, section, _)| {
            current_path == *href
                || current_path == *section
                || current_path.starts_with(&format!("{section}/"))
        })
        .map(|(href, _, _)| *href)
}

pub(super) fn page_document(
    title: &str,
    cookie_path: &str,
    current_path: &str,
    content: &str,
    scripts: &[&str],
) -> String {
    let current = current_navigation(current_path);
    // 移動先のリンク。現在の画面はaria-currentで示し、見た目もアクセント色の面で区別する。
    let destinations = NAVIGATION
        .iter()
        .map(|(href, _, label)| {
            let marker = if current == Some(*href) {
                " aria-current=\"page\""
            } else {
                ""
            };
            format!(
                "<li><a class=\"inline-flex min-h-11 w-full items-center rounded-sm px-3 py-2 text-sm font-semibold text-muted-foreground no-underline hover:bg-muted hover:text-foreground aria-[current=page]:bg-secondary aria-[current=page]:text-secondary-foreground\" href=\"{}\"{marker}>{label}</a></li>",
                escape_html(&external_path(cookie_path, href)),
            )
        })
        .collect::<String>();
    // 狭い画面では開閉するメニュー、広い画面では横に並べた移動先として表示する。
    // detailsとsummaryを使い、開閉にJavaScriptを必要としない。開閉の仕組みに関わる
    // 見た目(summaryの表示切替と::details-content)はlayout.cssのnavigation-*が受け持つ。
    let navigation = format!(
        "<details class=\"navigation-menu\"><summary class=\"navigation-menu-button\">メニュー</summary><ul class=\"navigation-list m-0 flex list-none items-center gap-1 p-0\">{destinations}</ul></details>",
    );
    let new_note = external_path(cookie_path, "/notes/new");
    let home = external_path(cookie_path, "/");
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
        "<!doctype html><html lang=\"ja\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>{}</title><link rel=\"stylesheet\" href=\"{}\">{}</head><body><header class=\"page-header sticky top-0 z-20 border-b bg-card/90 backdrop-blur-lg\"><div class=\"mx-auto flex min-h-16 max-w-(--content-width) flex-wrap items-center justify-between gap-x-3 gap-y-2 px-4 py-2 min-[60rem]:px-10\"><a class=\"brand inline-flex items-center gap-3 font-bold tracking-tight text-foreground no-underline\" href=\"{}\"><span class=\"grid size-8 place-items-center rounded-sm bg-primary text-sm text-primary-foreground\" aria-hidden=\"true\">M</span><span>Marginalis</span></a><nav class=\"flex items-center justify-end gap-2\" aria-label=\"主要な画面\">{}<a class=\"inline-flex h-9 shrink-0 items-center justify-center rounded-md bg-primary px-4 py-2 text-sm font-medium whitespace-nowrap text-primary-foreground no-underline hover:bg-primary/90\" href=\"{}\">新規ノート</a></nav></div></header><main class=\"page-main mx-auto max-w-(--content-width) px-4 pt-8 pb-14 min-[60rem]:px-10 min-[60rem]:pt-12 min-[60rem]:pb-20\">{}</main></body></html>",
        escape_html(title),
        escape_html(&stylesheet),
        scripts,
        escape_html(&home),
        navigation,
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
        let document = page_document("<題名>", "/marginalis", "/", "<h1>本文</h1>", &[]);

        assert!(document.contains("<html lang=\"ja\">"));
        assert!(document.contains("<title>&lt;題名&gt;</title>"));
        assert!(document.contains("href=\"/marginalis/assets/editor.css\""));
        assert!(!document.contains("<script"));
        assert!(document.contains("href=\"/marginalis/\""));
        assert!(document.contains("aria-label=\"主要な画面\""));
        assert!(document.contains("href=\"/marginalis/notes/new\""));
        assert!(document.contains("<h1>本文</h1></main>"));
        assert!(document.contains("class=\"page-main"));
        assert!(!document.contains("src=\"/marginalis/assets/editor.js\""));
    }

    /// 狭い画面でも移動先を隠さないため、主要な移動先は常にすべて出力する。
    #[test]
    fn page_document_lists_every_destination_and_marks_the_current_screen() {
        let document = page_document("Marginalis", "/", "/bibliography", "<div></div>", &[]);

        for label in ["ノート", "書誌", "関係の図", "設定", "新規ノート"] {
            assert!(document.contains(label), "{label}への移動手段が必要です");
        }
        assert!(document.contains("aria-current=\"page\">書誌</a>"));
        assert!(document.contains("href=\"/bibliography\" aria-current"));
        assert_eq!(document.matches("aria-current=\"page\"").count(), 1);

        // 個別のノートもノートの画面として示す。
        let note = page_document("Marginalis", "/", "/notes/abc", "<div></div>", &[]);
        assert!(note.contains("aria-current=\"page\">ノート</a>"));

        // 主要な移動先でない画面は、どれも現在位置として示さない。
        let consent = page_document("認可", "/", "/oauth/authorize", "<div></div>", &[]);
        assert!(!consent.contains("aria-current"));
    }

    #[test]
    fn page_document_can_load_a_subpath_module() {
        let document = page_document(
            "編集",
            "/marginalis",
            "/notes/new",
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
