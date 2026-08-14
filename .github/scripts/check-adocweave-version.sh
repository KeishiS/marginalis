#!/usr/bin/env bash
set -euo pipefail

# AdocWeaveの固定の正本は次の2箇所です。
#   revision: ルートCargo.tomlの[workspace.dependencies]にあるadocweave宣言のrev
#   版: そのrevisionからCargo.lockが解決したadocweave packageのversion
# この検査は、リポジトリ内に散在する参照が正本と一致していることを確かめます。
# flake.nixのcargoLock.outputHashesの鍵と、nix/checks/配下の移行検査が期待する版は
# Cargo.lockから導出するため、grepによる照合の対象はここに列挙した参照だけです。

temporary_directory=$(mktemp -d)
trap 'rm -rf "$temporary_directory"' EXIT

metadata="$temporary_directory/metadata.json"
metadata_input="${1:-}"
project_root="${2:-.}"

if [[ -n "$metadata_input" ]]; then
  cp "$metadata_input" "$metadata"
else
  cargo metadata --locked --format-version 1 >"$metadata"
fi

cd "$project_root"

fail() {
  echo "$1" >&2
  exit 1
}

# 1. 正本のrevisionをルートCargo.tomlから読み取ります。
declaration=$(grep -E '^adocweave *= *\{' Cargo.toml) ||
  fail "ルートCargo.tomlにadocweaveのworkspace依存宣言が見つかりません。"
expected_revision=$(printf '%s\n' "$declaration" | sed -En 's/.*rev = "([0-9a-f]+)".*/\1/p')
if [[ ! "$expected_revision" =~ ^[0-9a-f]{40}$ ]]; then
  fail "AdocWeaveのrevisionを40桁の完全SHAで固定してください: ${expected_revision:-（宣言なし）}"
fi

# 2. Cargo.lockの解決結果が正本のrevisionを指すことを確かめ、正本の版を読み取ります。
expected_version=$(jq -er --arg revision "$expected_revision" '
  [.packages[] | select(.name == "adocweave")] as $resolved
  | if ($resolved | length) != 1 then
      error("adocweave packageの解決結果が1件ではありません")
    elif (($resolved[0].source // "") | contains("rev=" + $revision) | not)
      or (($resolved[0].source // "") | endswith("#" + $revision) | not) then
      error("解決されたrevisionがCargo.tomlの宣言と一致しません")
    else
      $resolved[0].version
    end
' "$metadata") ||
  fail "Cargo.lockが解決したAdocWeaveがCargo.tomlの宣言と一致しません。"

# 3. Rust実装が公開する定数が、build.rsによるCargo.lock由来の導出であることを
#    確かめます。手書きのliteralへ戻ると、版上げのたびに更新漏れの余地が生まれます。
library=crates/marginalis-asciidoc/src/lib.rs
grep -Fq 'pub const ADOCWEAVE_SOURCE_REVISION: &str = env!("MARGINALIS_ADOCWEAVE_REVISION");' "$library" ||
  fail "$library のADOCWEAVE_SOURCE_REVISIONをbuild.rs導出のenv!参照にしてください。"
grep -Fq 'pub const PINNED_ADOCWEAVE_PACKAGE_VERSION: &str = env!("MARGINALIS_ADOCWEAVE_VERSION");' "$library" ||
  fail "$library のPINNED_ADOCWEAVE_PACKAGE_VERSIONをbuild.rs導出のenv!参照にしてください。"
test -f crates/marginalis-asciidoc/build.rs ||
  fail "crates/marginalis-asciidoc/build.rs がありません。"

# 4. flake.nixのinputが同じrevisionを指し、cargoハッシュの鍵をCargo.lockから導出して
#    いることを確かめます。ハッシュ値そのものの正しさはNixのbuildが検証します。
grep -Fq "url = \"github:KeishiS/adocweave/$expected_revision\";" flake.nix ||
  fail "flake.nixのadocweave inputが正本のrevisionと一致しません。"
grep -Fq '"adocweave-${adocweaveVersion}"' flake.nix ||
  fail "flake.nixのcargoLock.outputHashesの鍵をCargo.lock由来のadocweaveVersionから導出してください。"

# 5. textlint pluginのtarball URLを照合します。lockfileの解決結果も同じURLでなければ
#    なりません。
plugin_url="https://github.com/KeishiS/adocweave/releases/download/v$expected_version/adocweave-textlint-plugin-asciidoc-$expected_version.tgz"
jq -e --arg url "$plugin_url" \
  '.devDependencies["@adocweave/textlint-plugin-asciidoc"] == $url' \
  tools/textlint/package.json >/dev/null ||
  fail "tools/textlint/package.json のplugin URLが正本の版と一致しません。"
jq -e --arg url "$plugin_url" \
  '.packages["node_modules/@adocweave/textlint-plugin-asciidoc"].resolved == $url' \
  tools/textlint/package-lock.json >/dev/null ||
  fail "tools/textlint/package-lock.json のplugin解決先が正本の版と一致しません。"

# 6. 生成済みのOpenAPIとその生成元を照合します。
jq -e --arg version "$expected_version" \
  '.info["x-adocweave-package-version"] == $version' docs/openapi.json >/dev/null ||
  fail "docs/openapi.json のx-adocweave-package-versionが正本の版と一致しません。"
grep -Fq '"x-adocweave-package-version": env!("MARGINALIS_ADOCWEAVE_VERSION"),' \
  crates/marginalis-contract/src/rest.rs ||
  fail "crates/marginalis-contract/src/rest.rs のx-adocweave-package-versionをbuild.rs導出のenv!参照にしてください。"

# 7. archive CLIの結合試験が現行版を定数で参照していることを確かめます。旧版のliteralは
#    互換fixtureとして意図的に残ります。現行版のliteral直書きは更新漏れの元です。
grep -F 'adocweave_package_version' crates/marginalis-service/tests/cli.rs |
  grep -Fq 'PINNED_ADOCWEAVE_PACKAGE_VERSION' ||
  fail "crates/marginalis-service/tests/cli.rs の現行版参照はPINNED_ADOCWEAVE_PACKAGE_VERSION定数を使ってください。"
if grep -F 'adocweave_package_version' crates/marginalis-service/tests/cli.rs |
  grep -Fq "\"$expected_version\""; then
  fail "crates/marginalis-service/tests/cli.rs に現行版のliteral直書きが残っています。定数を使ってください。"
fi

# 8. 固定したrevisionがconformance fixtureを同梱していることを確かめます。
adocweave_manifest=$(jq -r --arg version "$expected_version" '
  .packages[]
  | select(.name == "adocweave" and .version == $version)
  | .manifest_path
' "$metadata")
test -f "$(dirname "$adocweave_manifest")/conformance/cases.json" ||
  fail "固定したAdocWeaveにconformance/cases.jsonが同梱されていません。"

# 9. 撤去済みの版識別子が復活していないことを確かめます。
if grep -Eq 'WASM_API_VERSION|PINNED_CONTRACTS|CONTRACT_VERSION|contractVersion' \
  "$library"; then
  fail "撤去済みのAdocWeave版識別子が $library に残っています。"
fi

echo "AdocWeaveの版とrevisionの一致を確認しました: v${expected_version} ${expected_revision}"
