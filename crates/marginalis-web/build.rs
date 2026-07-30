use std::{
    env, fs,
    path::{Path, PathBuf},
};

fn main() {
    let manifest_dir =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest directory"));
    let font_dir = manifest_dir
        .join("../../frontend/dist/assets/mathjax-fonts/mathjax-newcm-font/svg/dynamic");
    println!("cargo:rerun-if-changed={}", font_dir.display());

    let mut files = fs::read_dir(&font_dir)
        .unwrap_or_else(|error| {
            panic!("MathJaxの遅延字体が見つかりません。先にfrontendをビルドしてください: {error}")
        })
        .map(|entry| entry.expect("MathJax font directory entry").path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "js"))
        .collect::<Vec<_>>();
    files.sort();
    assert!(
        !files.is_empty(),
        "MathJaxの遅延字体が1件も見つかりません。"
    );

    let entries = files
        .iter()
        .map(|path| font_entry(&manifest_dir, path))
        .collect::<String>();
    let generated = format!("const MATHJAX_FONT_FILES: &[(&str, &[u8])] = &[\n{entries}];\n");
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("build output directory"))
        .join("mathjax_font_assets.rs");
    fs::write(output, generated).expect("write MathJax font asset list");
}

fn font_entry(manifest_dir: &Path, path: &Path) -> String {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .expect("UTF-8 MathJax font file name");
    let relative = path
        .strip_prefix(manifest_dir)
        .expect("MathJax font path below manifest directory")
        .to_str()
        .expect("UTF-8 MathJax font path")
        .replace('\\', "/");
    format!(
        "    ({name:?}, include_bytes!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/{relative}\"))),\n"
    )
}
