#!/usr/bin/env bash
set -euo pipefail

# release-manifest.jsonとrelease/notes.mdの整合を検査します。manifestは、公開する版、
# 構築に使うtoolchain、公開するasset、Release Notesの位置を機械可読に宣言する正本です。
# Release用Pull Requestの差分だけで公開内容を確認できるように、宣言と実際のファイルが
# 食い違う場合はここで拒否します。
#
# 使い方: check-release-manifest.sh [--with-runtime-toolchain] [プロジェクトの位置]
#
# ``--with-runtime-toolchain``を渡すと、宣言したNode.jsの版が実行中の開発環境と同じことも
# 確かめます。Nix開発環境の外(公開workflowなど)では、宣言どうしの整合だけを検査します。

runtime_toolchain=false
if [[ "${1:-}" == "--with-runtime-toolchain" ]]; then
  runtime_toolchain=true
  shift
fi
project_root="${1:-.}"

cd "$project_root"

fail() {
  echo "$1" >&2
  exit 1
}

manifest="release-manifest.json"
test -f "$manifest" || fail "${manifest}がありません。"
jq -e . "$manifest" >/dev/null 2>&1 || fail "${manifest}をJSONとして読めません。"

jq -e '
  (.schemaVersion == 1)
  and (.product | type == "string")
  and (.packageVersion | type == "string")
  and (.rustVersion | type == "string")
  and (.nodeVersion | type == "string")
  and (.releaseNotes | type == "string")
  and (.assets | type == "array")
' "$manifest" >/dev/null ||
  fail "${manifest}に必須項目schemaVersion、product、packageVersion、rustVersion、nodeVersion、releaseNotes、assetsが揃っていません。"

product="$(jq -er .product "$manifest")"
[[ "$product" == "marginalis" ]] || fail "manifestのproductがmarginalisではありません: ${product}"

package_version="$(jq -er .packageVersion "$manifest")"
if [[ ! "$package_version" =~ ^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]]; then
  fail "manifestのpackageVersionがMAJOR.MINOR.PATCH形式ではありません: ${package_version}"
fi

# Cargo workspaceの宣言を版とRustの正本として、manifestの写しと照合します。
workspace_field() {
  awk -v field="$1" '
    /^\[/ { in_section = ($0 == "[workspace.package]") ; next }
    in_section && $1 == field { print $3 ; exit }
  ' Cargo.toml | tr -d '"'
}

workspace_version="$(workspace_field version)"
[[ -n "$workspace_version" ]] || fail "Cargo.tomlの[workspace.package]にversionがありません。"
if [[ "$package_version" != "$workspace_version" ]]; then
  fail "manifestのpackageVersionがworkspaceの版と一致しません: manifest=${package_version}, workspace=${workspace_version}"
fi

workspace_rust="$(workspace_field rust-version)"
[[ -n "$workspace_rust" ]] || fail "Cargo.tomlの[workspace.package]にrust-versionがありません。"
manifest_rust="$(jq -er .rustVersion "$manifest")"
if [[ "$manifest_rust" != "$workspace_rust" ]]; then
  fail "manifestのrustVersionがCargo.tomlのrust-versionと一致しません: manifest=${manifest_rust}, Cargo.toml=${workspace_rust}"
fi

manifest_node="$(jq -er .nodeVersion "$manifest")"
if [[ ! "$manifest_node" =~ ^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]]; then
  fail "manifestのnodeVersionがMAJOR.MINOR.PATCH形式ではありません: ${manifest_node}"
fi
if [[ "$runtime_toolchain" == true ]]; then
  runtime_node="$(node --version)"
  runtime_node="${runtime_node#v}"
  if [[ "$manifest_node" != "$runtime_node" ]]; then
    fail "manifestのnodeVersionが開発環境のNode.jsと一致しません: manifest=${manifest_node}, 実行環境=${runtime_node}"
  fi
fi

asset_count="$(jq -er '.assets | length' "$manifest")"
((asset_count > 0)) || fail "manifestのassetsが空です。公開するassetを一つ以上宣言してください。"

jq -e 'all(.assets[]; (.name | type == "string") and (.source | type == "string"))' "$manifest" >/dev/null ||
  fail "manifestのassetsにはnameとsourceが必要です。"

duplicate_names="$(jq -r '[.assets[].name] | group_by(.) | map(select(length > 1) | .[0]) | .[]' "$manifest")"
[[ -z "$duplicate_names" ]] || fail "manifestのassetsに同じnameが複数あります: ${duplicate_names}"

while IFS=$'\t' read -r name source; do
  [[ "$name" != */* ]] || fail "assetのnameにディレクトリーを含められません: ${name}"
  test -f "$source" || fail "assetのsourceがありません: ${source}"
  [[ "${source##*/}" == "$name" ]] ||
    fail "assetのnameはsourceのファイル名と同じにしてください: name=${name}, source=${source}"
done < <(jq -r '.assets[] | [.name, .source] | @tsv' "$manifest")

notes="$(jq -er .releaseNotes "$manifest")"
test -f "$notes" || fail "manifestが指すRelease Notesがありません: ${notes}"

expected_title="# Marginalis v${package_version}"
actual_title="$(head -n 1 "$notes")"
if [[ "$actual_title" != "$expected_title" ]]; then
  fail "Release Notesの題名が公開する版と一致しません: expected=${expected_title}, actual=${actual_title}"
fi

# 公開のたびに読み手が同じ観点を確認できるように、必須の見出しをこの順で要求します。
required_headings=(
  "## 主な変更"
  "## 対応環境"
  "## 公開契約と破壊的変更"
  "## v${package_version}への移行"
  "## 更新とロールバック"
  "## 既知の制約"
  "## 配布物の検証"
)

previous_line=0
for heading in "${required_headings[@]}"; do
  mapfile -t matches < <(grep -nFx -- "$heading" "$notes" || true)
  if [[ "${#matches[@]}" -ne 1 ]]; then
    fail "Release Notesに必須の見出しが一つだけ現れていません: ${heading}"
  fi
  current_line="${matches[0]%%:*}"
  if ((current_line <= previous_line)); then
    fail "Release Notesの見出しの順序が正しくありません: ${heading}"
  fi
  previous_line="$current_line"
done

if grep -qE 'TODO|FIXME|記載してから公開' "$notes"; then
  fail "Release Notesに未記入の目印が残っています: ${notes}"
fi

# 見出しだけがあって本文がない節は、公開前に気付けないまま残ります。
missing_body="$(awk '
  /^## / {
    if (heading != "" && body == 0) { print heading }
    heading = $0
    body = 0
    next
  }
  /^[[:space:]]*$/ { next }
  { if (heading != "") body = 1 }
  END { if (heading != "" && body == 0) print heading }
' "$notes")"
[[ -z "$missing_body" ]] || fail "Release Notesに本文のない節があります: ${missing_body}"

echo "release manifestとRelease Notesの整合を確認しました: v${package_version}"
