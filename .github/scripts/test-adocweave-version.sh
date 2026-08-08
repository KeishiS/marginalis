#!/usr/bin/env bash
set -euo pipefail

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
work_dir=$(mktemp -d)
trap 'rm -rf "$work_dir"' EXIT

revision=1111111111111111111111111111111111111111
other_revision=2222222222222222222222222222222222222222
version=0.40.0
other_version=0.41.0

# 検査対象の参照を最小構成で再現したリポジトリを組み立てます。
build_tree() {
  local root=$1 tree_version=$2 tree_revision=$3
  local plugin_url="https://github.com/KeishiS/adocweave/releases/download/v$tree_version/adocweave-textlint-plugin-asciidoc-$tree_version.tgz"
  mkdir -p "$root"/{crates/marginalis-asciidoc/src,crates/marginalis-contract/src,crates/marginalis-service/tests,docs,tools/textlint}
  cat >"$root/Cargo.toml" <<EOF
[workspace.dependencies]
adocweave = { git = "https://github.com/KeishiS/adocweave.git", rev = "$tree_revision", package = "adocweave" }
EOF
  cat >"$root/crates/marginalis-asciidoc/src/lib.rs" <<EOF
pub const ADOCWEAVE_SOURCE_REVISION: &str = "$tree_revision";
pub const PINNED_ADOCWEAVE_PACKAGE_VERSION: &str = "$tree_version";
EOF
  cat >"$root/flake.nix" <<EOF
  inputs.adocweave = {
    url = "github:KeishiS/adocweave/$tree_revision";
  };
  outputHashes = {
    "adocweave-\${adocweaveVersion}" = "sha256-0000000000000000000000000000000000000000000=";
  };
EOF
  jq -n --arg url "$plugin_url" \
    '{devDependencies: {"@adocweave/textlint-plugin-asciidoc": $url}}' \
    >"$root/tools/textlint/package.json"
  jq -n --arg url "$plugin_url" \
    '{packages: {"node_modules/@adocweave/textlint-plugin-asciidoc": {resolved: $url}}}' \
    >"$root/tools/textlint/package-lock.json"
  jq -n --arg version "$tree_version" \
    '{info: {"x-adocweave-package-version": $version}}' >"$root/docs/openapi.json"
  cat >"$root/crates/marginalis-contract/src/rest.rs" <<EOF
            "x-adocweave-package-version": "$tree_version",
EOF
  cat >"$root/crates/marginalis-service/tests/cli.rs" <<EOF
    assert_eq!(archive_json["adocweave_package_version"], "$tree_version");
    incompatible_json["adocweave_package_version"] = "0.10.1".into();
EOF
  mkdir -p "$root/adocweave-checkout/conformance"
  echo '[]' >"$root/adocweave-checkout/conformance/cases.json"
  jq -n \
    --arg version "$tree_version" \
    --arg source "git+https://github.com/KeishiS/adocweave.git?rev=$tree_revision#$tree_revision" \
    --arg manifest "$root/adocweave-checkout/Cargo.toml" \
    '{packages: [{name: "adocweave", version: $version, source: $source, manifest_path: $manifest}]}' \
    >"$root/metadata.json"
}

build_tree "$work_dir/good" "$version" "$revision"
bash "$script_dir/check-adocweave-version.sh" \
  "$work_dir/good/metadata.json" "$work_dir/good" >/dev/null

reject() {
  local description=$1
  if bash "$script_dir/check-adocweave-version.sh" \
    "$work_dir/bad/metadata.json" "$work_dir/bad" >/dev/null 2>&1; then
    echo "受理してはいけない状態を受理しました: $description" >&2
    exit 1
  fi
}

rebuild() {
  rm -rf "$work_dir/bad"
  build_tree "$work_dir/bad" "$version" "$revision"
}

# Cargo.tomlのrevisionが短縮形の場合
rebuild
sed -i "s/$revision/1111111/" "$work_dir/bad/Cargo.toml"
reject "短縮revision"

# Cargo.lockの解決結果が別のrevisionを指す場合
rebuild
jq --arg source "git+https://github.com/KeishiS/adocweave.git?rev=$other_revision#$other_revision" \
  '.packages[0].source = $source' \
  "$work_dir/bad/metadata.json" >"$work_dir/bad/metadata.new"
mv "$work_dir/bad/metadata.new" "$work_dir/bad/metadata.json"
reject "解決revisionの不一致"

# Cargo.lockの解決版だけが変わり、参照側が古い版のままの場合
rebuild
jq --arg version "$other_version" '.packages[0].version = $version' \
  "$work_dir/bad/metadata.json" >"$work_dir/bad/metadata.new"
mv "$work_dir/bad/metadata.new" "$work_dir/bad/metadata.json"
reject "参照側の版の残置"

# Rust定数のrevisionが古い場合
rebuild
sed -i "s/$revision/$other_revision/" "$work_dir/bad/crates/marginalis-asciidoc/src/lib.rs"
reject "Rust定数のrevision不一致"

# Rust定数の版が古い場合
rebuild
sed -i "s/PINNED_ADOCWEAVE_PACKAGE_VERSION: \&str = \"$version\"/PINNED_ADOCWEAVE_PACKAGE_VERSION: \&str = \"$other_version\"/" \
  "$work_dir/bad/crates/marginalis-asciidoc/src/lib.rs"
reject "Rust定数の版不一致"

# flake inputのrevisionが古い場合
rebuild
sed -i "s/$revision/$other_revision/" "$work_dir/bad/flake.nix"
reject "flake inputのrevision不一致"

# flakeのcargoハッシュ鍵が版のliteral直書きへ戻った場合
rebuild
sed -i 's/adocweave-${adocweaveVersion}/adocweave-0.40.0/' "$work_dir/bad/flake.nix"
reject "cargoハッシュ鍵のliteral直書き"

# textlint pluginのtarball URLが古い場合
rebuild
sed -i "s/$version/$other_version/g" "$work_dir/bad/tools/textlint/package.json"
reject "plugin URLの版不一致"

# textlintのlockfileの解決先が古い場合
rebuild
sed -i "s/$version/$other_version/g" "$work_dir/bad/tools/textlint/package-lock.json"
reject "plugin lockfileの版不一致"

# 生成済みOpenAPIの版が古い場合
rebuild
sed -i "s/$version/$other_version/" "$work_dir/bad/docs/openapi.json"
reject "OpenAPIの版不一致"

# OpenAPI生成元の版が古い場合
rebuild
sed -i "s/$version/$other_version/" "$work_dir/bad/crates/marginalis-contract/src/rest.rs"
reject "OpenAPI生成元の版不一致"

# 結合試験に現行版の参照がない場合
rebuild
sed -i "s/$version/$other_version/" "$work_dir/bad/crates/marginalis-service/tests/cli.rs"
reject "結合試験の現行版参照の欠落"

# conformance fixtureが同梱されていない場合
rebuild
rm "$work_dir/bad/adocweave-checkout/conformance/cases.json"
reject "conformance fixtureの欠落"

# 撤去済みの版識別子が復活した場合
rebuild
echo 'pub const WASM_API_VERSION: &str = "1";' \
  >>"$work_dir/bad/crates/marginalis-asciidoc/src/lib.rs"
reject "撤去済み版識別子の復活"

echo "AdocWeave版数検査の自己テストに成功しました。"
