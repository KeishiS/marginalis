//! AdocWeaveの版をCargo.lockの解決結果から導出する。
//!
//! OpenAPIの`x-adocweave-package-version`が参照する。crate間の依存境界により
//! marginalis-asciidocへは依存できないため、同じ導出をこのcrateでも行う
//! (処理の本体はmarginalis-asciidoc/build.rsと同じ規則)。

use std::path::Path;

fn main() {
    let lock_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.lock");
    println!("cargo:rerun-if-changed={}", lock_path.display());
    let lock = std::fs::read_to_string(&lock_path).expect("ワークスペースのCargo.lockを読める");
    let package = lock
        .split("[[package]]")
        .find(|block| block.contains("name = \"adocweave\""))
        .expect("Cargo.lockにadocweave packageがある");
    let version = package
        .lines()
        .find_map(|line| line.strip_prefix("version = \""))
        .and_then(|rest| rest.strip_suffix('"'))
        .expect("adocweave packageにversionがある");
    println!("cargo:rustc-env=MARGINALIS_ADOCWEAVE_VERSION={version}");
}
