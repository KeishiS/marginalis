#!/usr/bin/env bash
set -euo pipefail

# AdocWeaveの固定を新しい版へ更新します。
#
#   使い方: bash .github/scripts/bump-adocweave.sh <adocweave-coreのcommit SHA(40桁)> [textlint用Processorの版]
#
# Native製品とRustライブラリは同じ版で公開されます。textlint用Processorだけは独立した版を
# 持つため、別の引数で指定します。textlintの版を省略した場合は、その更新を行いません。
#
# 正本(Cargo.tomlのrevとCargo.lockの解決結果)を更新すると、Rust定数とOpenAPIの版はbuild.rsが
# 導出するため、このスクリプトはそれ以外の機械的に決まる参照を追随させます。
#   - flake.nixのinput URL・cargoLock.outputHashesのハッシュ・flake.lock
#   - tools/textlintのplugin版とlockfile
#   - 生成済みのOpenAPI(x-adocweave-package-version)
#
# 文書中の版表記は書き換えません。docs/user-guide/nixos.adocには保存契約の履歴表があり、過去の
# 契約が使うAdocWeave版が並んでいます。旧版の文字列を一括置換すると、現行版の説明だけでなく
# 履歴の行まで書き換わります。最後に、旧版を含む行を一覧で示すので、現行版を指す箇所だけを
# 更新してください。archiveの移行契約も履歴の追加が必要なため、同じく案内します。

revision="${1:-}"
textlint_version="${2:-}"
if [[ ! "$revision" =~ ^[0-9a-f]{40}$ ]]; then
  echo "使い方: bash .github/scripts/bump-adocweave.sh <adocweave-coreのcommit SHA(40桁)> [textlint用Processorの版]" >&2
  exit 1
fi
if [[ -n "$textlint_version" && ! "$textlint_version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "textlint用Processorの版はMAJOR.MINOR.PATCHで指定してください: $textlint_version" >&2
  exit 1
fi

old_revision=$(sed -En 's/^adocweave-core *= *\{.*rev = "([0-9a-f]{40})".*/\1/p' Cargo.toml)
old_version=$(sed -n '/name = "adocweave-core"/{n;s/^version = "\(.*\)"$/\1/p;}' Cargo.lock)
if [[ -z "$old_revision" || -z "$old_version" ]]; then
  echo "現在の固定をCargo.tomlとCargo.lockから読み取れません。" >&2
  exit 1
fi

echo "1/5 Cargo.tomlのrevを更新してCargo.lockを解決します。"
sed -i "s/$old_revision/$revision/" Cargo.toml
cargo update -p adocweave-core
new_version=$(sed -n '/name = "adocweave-core"/{n;s/^version = "\(.*\)"$/\1/p;}' Cargo.lock)
echo "    ライブラリ v$old_version ($old_revision) -> v$new_version ($revision)"

echo "2/5 flake.nixのinputとハッシュ、flake.lockを更新します。"
new_hash=$(nix flake prefetch "github:KeishiS/adocweave/$revision" --json | jq -r .hash)
old_hash=$(sed -En 's/.*"adocweave-core-\$\{adocweaveVersion\}" = "([^"]+)";/\1/p' flake.nix)
# Nixのハッシュはbase64で/を含むため、置換の区切りへ|を使う。base64と16進SHAはどちらも
# |を含まないので、区切りが値と衝突することはない。
sed -i "s|$old_revision|$revision|; s|$old_hash|$new_hash|" flake.nix
nix flake lock --update-input adocweave

if [[ -n "$textlint_version" ]]; then
  echo "3/5 textlint用Processorをv${textlint_version}へ更新します。"
  (cd tools/textlint &&
    npm install --save-exact --save-dev --no-audit --no-fund \
      "@adocweave/textlint-plugin-asciidoc@${textlint_version}" >/dev/null)
else
  echo "3/5 textlint用Processorの版指定がないため、更新を省略します。"
fi

echo "4/5 契約の生成物を更新します。"
cargo run --quiet -p marginalis-contract --bin generate

echo "5/5 一貫性を検査します。"
bash .github/scripts/check-adocweave-version.sh

echo
echo "更新が完了しました: ライブラリ v$old_version -> v$new_version"
echo
echo "旧版($old_version)を含む文書の行です。現行版を指す箇所だけを更新してください。"
echo "保存契約の履歴表にある過去の契約は、そのまま残します。"
rg --line-number --fixed-strings "$old_version" docs || echo "  (該当なし)"

cat <<'GUIDE'

残る手作業:
  - crates/marginalis-archive/src/archive.rs のPREVIOUS_MIGRATION_CONTRACTを、更新前の
    archive形式、AdocWeave package版、note profile版の組へ更新してください。
  - docs/user-guide/nixos.adocの保存契約履歴へ、更新後の契約を新しい行として追加し、
    直前契約と段階移行の欄を繰り上げてください。
  - cargo make verify を実行してください。
GUIDE
