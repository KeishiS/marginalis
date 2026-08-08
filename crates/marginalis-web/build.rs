use std::{
    env, fs,
    path::{Path, PathBuf},
};

fn main() {
    let manifest_dir =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest directory"));
    write_asset_list(
        &manifest_dir,
        "../../frontend/dist/assets/mathjax-fonts/mathjax-newcm-font/svg/dynamic",
        "js",
        "MathJaxの遅延字体",
        "MATHJAX_FONT_FILES",
        "mathjax_font_assets.rs",
    );
    write_asset_list(
        &manifest_dir,
        "../../frontend/dist/assets/fonts",
        "woff2",
        "Web字体",
        "WEB_FONT_FILES",
        "web_font_assets.rs",
    );
    // Viteの出力名を列挙せずにまとめて埋め込む。分割読み込みで増えるchunkは名前へhashが付き、
    // 内容が変わるたびに変わる。名前を一つずつ書くと、新しい出力が配信されないまま気付けない。
    write_bundle_list(&manifest_dir, "../../frontend/dist/assets");
}

/// `dist/assets`直下のファイルを、名前と内容の表として埋め込む。
///
/// 部分ディレクトリー（字体など）はそれぞれ専用の表を持つため、ここでは通常ファイルだけを見る。
fn write_bundle_list(manifest_dir: &Path, relative_directory: &str) {
    let directory = manifest_dir.join(relative_directory);
    println!("cargo:rerun-if-changed={}", directory.display());
    let mut files = fs::read_dir(&directory)
        .unwrap_or_else(|error| {
            panic!("Web UIの配布物が見つかりません。先にfrontendをビルドしてください: {error}")
        })
        .map(|entry| entry.expect("asset directory entry").path())
        .filter(|path| path.is_file())
        .collect::<Vec<_>>();
    files.sort();
    assert!(!files.is_empty(), "Web UIの配布物が1件も見つかりません。");
    let entries = files
        .iter()
        .map(|path| {
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .expect("UTF-8 asset file name");
            let content_type = content_type_for(name);
            let relative = path
                .strip_prefix(manifest_dir)
                .expect("asset path below manifest directory")
                .to_str()
                .expect("UTF-8 asset path")
                .replace('\\', "/");
            format!(
                "    ({name:?}, {content_type:?}, include_bytes!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/{relative}\"))),\n"
            )
        })
        .collect::<String>();
    let generated = format!(
        "/// 埋め込んだ`dist/assets`直下の(名前, MIME type, 内容)。試験も配布物と経路の対応確認に読む。\npub(crate) const BUNDLE_FILES: &[(&str, &str, &[u8])] = &[\n{entries}];\n"
    );
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("build output directory"))
        .join("bundle_assets.rs");
    fs::write(output, generated).expect("write embedded bundle list");
}

/// 拡張子から配信するMIME typeを決める。
///
/// 未知の拡張子はビルドを止める。誤ったMIME typeで配信すると、ブラウザーはmoduleとして
/// 読み込まず、画面が黙って壊れる。
fn content_type_for(name: &str) -> &'static str {
    match name.rsplit_once('.').map(|(_, extension)| extension) {
        Some("js") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json") => "application/json",
        Some("map") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("woff2") => "font/woff2",
        other => panic!(
            "配信するMIME typeが決まっていない配布物です: {name} (拡張子 {other:?})。\
             build.rsのcontent_type_forへ追加してください。"
        ),
    }
}

fn write_asset_list(
    manifest_dir: &Path,
    relative_directory: &str,
    extension: &str,
    label: &str,
    constant_name: &str,
    output_name: &str,
) {
    let directory = manifest_dir.join(relative_directory);
    println!("cargo:rerun-if-changed={}", directory.display());
    let mut files = fs::read_dir(&directory)
        .unwrap_or_else(|error| {
            panic!("{label}が見つかりません。先にfrontendをビルドしてください: {error}")
        })
        .map(|entry| entry.expect("asset directory entry").path())
        .filter(|path| {
            path.extension()
                .is_some_and(|candidate| candidate == extension)
        })
        .collect::<Vec<_>>();
    files.sort();
    assert!(!files.is_empty(), "{label}が1件も見つかりません。");
    let entries = files
        .iter()
        .map(|path| asset_entry(manifest_dir, path))
        .collect::<String>();
    let generated = format!("const {constant_name}: &[(&str, &[u8])] = &[\n{entries}];\n");
    let output =
        PathBuf::from(env::var_os("OUT_DIR").expect("build output directory")).join(output_name);
    fs::write(output, generated).expect("write embedded asset list");
}

fn asset_entry(manifest_dir: &Path, path: &Path) -> String {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .expect("UTF-8 asset file name");
    let relative = path
        .strip_prefix(manifest_dir)
        .expect("asset path below manifest directory")
        .to_str()
        .expect("UTF-8 asset path")
        .replace('\\', "/");
    format!(
        "    ({name:?}, include_bytes!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/{relative}\"))),\n"
    )
}
