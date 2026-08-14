#!/usr/bin/env bash
set -euo pipefail

# AdocWeaveの固定を新しい版へ一括更新します。
#
#   使い方: bash .github/scripts/bump-adocweave.sh <commit SHA(40桁)>
#
# 正本(Cargo.tomlのrevとCargo.lockの解決結果)を更新すると、Rust定数とOpenAPIの版は
# build.rsが導出するため、このスクリプトはそれ以外の機械的に決まる参照を追随させます。
#   - flake.nixのinput URL・cargoLock.outputHashesのハッシュ・flake.lock
#   - tools/textlintのplugin tarball URLとlockfile
#   - 文書中の現行版の表記
#   - 生成済みのOpenAPI(x-adocweave-package-version)
# archiveの移行契約(旧版の受理行)は履歴の追加が必要なため、最後に案内します。

revision="${1:-}"
if [[ ! "$revision" =~ ^[0-9a-f]{40}$ ]]; then
  echo "使い方: bash .github/scripts/bump-adocweave.sh <commit SHA(40桁)>" >&2
  exit 1
fi

old_revision=$(sed -En 's/^adocweave *= *\{.*rev = "([0-9a-f]{40})".*/\1/p' Cargo.toml)
old_version=$(sed -n '/name = "adocweave"/{n;s/^version = "\(.*\)"$/\1/p;}' Cargo.lock)
if [[ -z "$old_revision" || -z "$old_version" ]]; then
  echo "現在の固定をCargo.tomlとCargo.lockから読み取れません。" >&2
  exit 1
fi

echo "1/6 Cargo.tomlのrevを更新してCargo.lockを解決します。"
perl -pi -e "s/$old_revision/$revision/" Cargo.toml
cargo update -p adocweave
new_version=$(sed -n '/name = "adocweave"/{n;s/^version = "\(.*\)"$/\1/p;}' Cargo.lock)
echo "    v$old_version ($old_revision) -> v$new_version ($revision)"

echo "2/6 flake.nixのinputとハッシュ、flake.lockを更新します。"
new_hash=$(nix flake prefetch "github:KeishiS/adocweave/$revision" --json | jq -r .hash)
old_hash=$(sed -En 's/.*"adocweave-\$\{adocweaveVersion\}" = "([^"]+)";/\1/p' flake.nix)
# ハッシュには/が含まれるため、置換の区切りへ{}を使う。
perl -pi -e "s{\Q$old_revision\E}{$revision}; s{\Q$old_hash\E}{$new_hash}" flake.nix
nix flake lock --update-input adocweave

echo "3/6 textlint pluginのtarball URLとlockfileを更新します。"
perl -pi -e "s/\Qv$old_version\E/v$new_version/g; s/\Qasciidoc-$old_version.tgz\E/asciidoc-$new_version.tgz/g" \
  tools/textlint/package.json
(cd tools/textlint && npm install --no-audit --no-fund >/dev/null)

echo "4/6 文書中の現行版の表記を更新します。"
perl -pi -e "s/\Q$old_version\E/$new_version/g" \
  docs/user-guide/nixos.adoc \
  docs/developer-guide/requirements.adoc \
  docs/developer-guide/documentation.adoc

echo "5/6 契約の生成物を更新します。"
cargo run --quiet -p marginalis-contract --bin generate

echo "6/6 一貫性を検査します。"
bash .github/scripts/check-adocweave-version.sh

cat <<GUIDE

更新が完了しました: v$old_version -> v$new_version

残る手作業:
  - crates/marginalis-archive/src/lib.rs のSUPPORTED_MIGRATION_CONTRACTSへ
    旧版の受理行 migration_contract("marginalis-archive-*", "$old_version", ...) を追加し、
    試験のLATEST_MIGRATION_CONTRACTも旧版へ更新してください。
  - cargo make verify を実行してください。
GUIDE
