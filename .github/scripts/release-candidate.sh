#!/usr/bin/env bash
set -euo pipefail

# release candidateは、公開予定の内容を一つのcommitへ固定した集合です。mainのCIが作り、
# stable release workflowが作り直さずにそのまま公開します。中身は次のとおりです。
#
#   - release-manifest.jsonが宣言した公開asset
#   - Release Notesの本文(notes.md)
#   - source commit、版、tag、toolchain、各fileのsizeとSHA-256を記録したrelease-candidate.json
#
# generateは候補を作り、verifyは候補が作業treeの内容と一致することを確かめます。verifyは
# 公開直前にも実行するため、候補のbytesとtagが指すcommitの内容が食い違ったまま公開されません。
#
# 使い方:
#   release-candidate.sh generate <候補ディレクトリー> <source commit> [プロジェクトの位置]
#   release-candidate.sh verify   <候補ディレクトリー> <source commit> [プロジェクトの位置]

command="${1:-}"
directory="${2:-}"
source_commit="${3:-}"
project_root="${4:-.}"

fail() {
  echo "$1" >&2
  exit 1
}

case "$command" in
  generate | verify) ;;
  *) fail "使い方: release-candidate.sh generate|verify <候補ディレクトリー> <source commit> [プロジェクトの位置]" ;;
esac

[[ -n "$directory" ]] || fail "候補ディレクトリーを指定してください。"
[[ "$source_commit" =~ ^[0-9a-f]{40}$ ]] ||
  fail "source commitは小文字40桁のGit commitで指定してください: ${source_commit}"

directory="$(CDPATH= cd -- "$(dirname -- "$directory")" && pwd)/$(basename -- "$directory")"
cd "$project_root"

manifest="release-manifest.json"
test -f "$manifest" || fail "${manifest}がありません。"

package_version="$(jq -er .packageVersion "$manifest")"
product="$(jq -er .product "$manifest")"
notes_source="$(jq -er .releaseNotes "$manifest")"
notes_name="notes.md"
metadata_name="release-candidate.json"

digest() {
  sha256sum "$1" | cut -d' ' -f1
}

file_entry() {
  local name=$1
  local path=$2
  test -f "$path" || fail "候補に必要なファイルがありません: ${path}"
  jq -n --arg name "$name" \
    --argjson size "$(wc -c <"$path")" \
    --arg sha256 "$(digest "$path")" \
    '{name: $name, size: $size, sha256: $sha256}'
}

if [[ "$command" == generate ]]; then
  mkdir -p "$directory"
  while IFS=$'\t' read -r name source; do
    test -f "$source" || fail "assetのsourceがありません: ${source}"
    cp -- "$source" "$directory/$name"
  done < <(jq -r '.assets[] | [.name, .source] | @tsv' "$manifest")
  cp -- "$notes_source" "$directory/$notes_name"
fi

# 期待するmetadataは、候補ディレクトリーではなく作業treeの内容から組み立てます。
# これにより、verifyは候補のbytesがcommitの内容と同じであることを検査できます。
assets_metadata="$(jq -n '[]')"
while IFS=$'\t' read -r name source; do
  entry="$(file_entry "$name" "$source")"
  assets_metadata="$(jq --argjson entry "$entry" '. + [$entry]' <<<"$assets_metadata")"
done < <(jq -r '.assets[] | [.name, .source] | @tsv' "$manifest")

notes_metadata="$(file_entry "$notes_name" "$notes_source")"

expected_metadata="$(mktemp)"
trap 'rm -f "$expected_metadata"' EXIT
jq -S -n \
  --arg product "$product" \
  --arg version "$package_version" \
  --arg tag "v${package_version}" \
  --arg sourceCommit "$source_commit" \
  --arg rustVersion "$(jq -er .rustVersion "$manifest")" \
  --arg nodeVersion "$(jq -er .nodeVersion "$manifest")" \
  --argjson releaseNotes "$notes_metadata" \
  --argjson assets "$assets_metadata" \
  '{
    schemaVersion: 1,
    product: $product,
    version: $version,
    tag: $tag,
    sourceCommit: $sourceCommit,
    rustVersion: $rustVersion,
    nodeVersion: $nodeVersion,
    releaseNotes: $releaseNotes,
    assets: ($assets | sort_by(.name))
  }' >"$expected_metadata"

if [[ "$command" == generate ]]; then
  install -m 644 -- "$expected_metadata" "$directory/$metadata_name"
  echo "release candidateを作成しました: v${package_version} ${source_commit}"
  exit 0
fi

test -f "$directory/$metadata_name" || fail "候補に${metadata_name}がありません。"
cmp -s "$expected_metadata" "$directory/$metadata_name" ||
  fail "候補の${metadata_name}が作業treeの内容と一致しません。"

# 記録したsizeとSHA-256で、候補ディレクトリーのbytesそのものを検査します。
while IFS=$'\t' read -r name size sha256; do
  path="$directory/$name"
  test -f "$path" || fail "候補にファイルがありません: ${name}"
  actual_size="$(wc -c <"$path")"
  [[ "$actual_size" == "$size" ]] ||
    fail "候補のsizeが一致しません: ${name} expected=${size}, actual=${actual_size}"
  actual_sha256="$(digest "$path")"
  [[ "$actual_sha256" == "$sha256" ]] ||
    fail "候補のSHA-256が一致しません: ${name}"
done < <(jq -r '[.releaseNotes] + .assets | .[] | [.name, .size, .sha256] | @tsv' "$expected_metadata")

expected_files="$(jq -r '([.releaseNotes.name] + [.assets[].name]) | .[]' "$expected_metadata" |
  { cat; echo "$metadata_name"; } | LC_ALL=C sort)"
actual_files="$(find "$directory" -mindepth 1 -maxdepth 1 -printf '%y\t%f\n' |
  awk -F'\t' '{ if ($1 != "f") { print "候補にファイル以外が含まれています: " $2 > "/dev/stderr"; exit 1 } print $2 }' |
  LC_ALL=C sort)"
if [[ "$expected_files" != "$actual_files" ]]; then
  diff -u <(printf '%s\n' "$expected_files") <(printf '%s\n' "$actual_files") >&2 || true
  fail "候補のファイル集合が宣言と一致しません。"
fi

echo "release candidateを検証しました: v${package_version} ${source_commit}"
