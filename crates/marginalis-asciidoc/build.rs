//! AdocWeaveの版とrevisionをCargo.lockの解決結果から導出する。
//!
//! 正本はルートCargo.tomlのrev宣言と、それをCargo.lockが解決した結果である。
//! 版数を手書きの定数として持つと更新漏れが起きるため、build時に読み取って
//! 環境変数で公開し、`lib.rs`の定数は`env!`で受け取る。

use std::path::Path;

fn main() {
    let lock_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.lock");
    println!("cargo:rerun-if-changed={}", lock_path.display());
    let lock = std::fs::read_to_string(&lock_path).expect("ワークスペースのCargo.lockを読める");
    let (version, revision) = adocweave_resolution(&lock);
    println!("cargo:rustc-env=MARGINALIS_ADOCWEAVE_VERSION={version}");
    println!("cargo:rustc-env=MARGINALIS_ADOCWEAVE_REVISION={revision}");
}

/// Cargo.lockのadocweave packageから、versionとgit revisionを読み取る。
///
/// 依存を増やさないため、TOML parserではなく`[[package]]`区切りの文字列処理で読む。
/// Cargo.lockの形式が変わって読めない場合はbuildを失敗させ、静かな不一致を残さない。
fn adocweave_resolution(lock: &str) -> (String, String) {
    let package = lock
        .split("[[package]]")
        .find(|block| block.contains("name = \"adocweave\""))
        .expect("Cargo.lockにadocweave packageがある");
    let field = |name: &str| {
        package
            .lines()
            .find_map(|line| line.strip_prefix(&format!("{name} = \"")))
            .and_then(|rest| rest.strip_suffix('"'))
            .unwrap_or_else(|| panic!("adocweave packageに{name}がある"))
            .to_owned()
    };
    let source = field("source");
    let revision = source
        .rsplit_once('#')
        .map(|(_, revision)| revision.to_owned())
        .filter(|revision| {
            revision.len() == 40 && revision.bytes().all(|byte| byte.is_ascii_hexdigit())
        })
        .expect("adocweaveのsourceが完全SHAのgit固定である");
    (field("version"), revision)
}
