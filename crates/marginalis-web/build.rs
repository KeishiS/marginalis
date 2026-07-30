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
