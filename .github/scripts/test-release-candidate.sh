#!/usr/bin/env bash
set -euo pipefail

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
work_dir=$(mktemp -d)
trap 'rm -rf "$work_dir"' EXIT

commit="0123456789abcdef0123456789abcdef01234567"
other_commit="89abcdef0123456789abcdef0123456789abcdef"

build_tree() {
  local root=$1
  mkdir -p "$root/docs" "$root/release"
  jq -n '{
    schemaVersion: 1,
    product: "marginalis",
    packageVersion: "1.2.3",
    rustVersion: "1.97.1",
    nodeVersion: "22.23.1",
    releaseNotes: "release/notes.md",
    assets: [
      {name: "mcp-tools.json", source: "docs/mcp-tools.json"},
      {name: "openapi.json", source: "docs/openapi.json"}
    ]
  }' >"$root/release-manifest.json"
  echo '{"tools":[]}' >"$root/docs/mcp-tools.json"
  echo '{"openapi":"3.1.0"}' >"$root/docs/openapi.json"
  printf '# Marginalis v1.2.3\n\n## 主な変更\n\n変更はありません。\n' >"$root/release/notes.md"
}

generate() {
  bash "$script_dir/release-candidate.sh" generate "$1/candidate" "$commit" "$1" >/dev/null
}

verify() {
  bash "$script_dir/release-candidate.sh" verify "$1/candidate" "${2:-$commit}" "$1" >/dev/null
}

reject() {
  local description=$1
  if verify "$work_dir/bad" "${2:-$commit}" 2>/dev/null; then
    echo "受理してはいけない状態を受理しました: $description" >&2
    exit 1
  fi
}

rebuild() {
  rm -rf "$work_dir/bad"
  build_tree "$work_dir/bad"
  generate "$work_dir/bad"
}

build_tree "$work_dir/good"
generate "$work_dir/good"
verify "$work_dir/good"

# 候補は宣言したassetとRelease Notes、metadataだけを含みます。
expected_files=$(printf 'mcp-tools.json\nnotes.md\nopenapi.json\nrelease-candidate.json\n')
actual_files=$(find "$work_dir/good/candidate" -mindepth 1 -maxdepth 1 -printf '%f\n' | LC_ALL=C sort)
if [[ "$expected_files" != "$actual_files" ]]; then
  echo "候補のファイル集合が想定と異なります。" >&2
  diff -u <(printf '%s\n' "$expected_files") <(printf '%s\n' "$actual_files") >&2 || true
  exit 1
fi

# 同じ入力からは同じmetadataができ、二度目の生成でも内容が変わりません。
cp "$work_dir/good/candidate/release-candidate.json" "$work_dir/first.json"
generate "$work_dir/good"
cmp -s "$work_dir/first.json" "$work_dir/good/candidate/release-candidate.json" || {
  echo "同じ入力から同じ候補metadataができません。" >&2
  exit 1
}

# 公開assetが候補から欠けた場合
rebuild
rm "$work_dir/bad/candidate/openapi.json"
reject "assetの欠落"

# 候補のassetが書き換えられた場合
rebuild
echo '{"openapi":"3.0.0"}' >"$work_dir/bad/candidate/openapi.json"
reject "assetの書き換え"

# 候補のRelease Notesが書き換えられた場合
rebuild
printf '# Marginalis v1.2.3\n' >"$work_dir/bad/candidate/notes.md"
reject "Release Notesの書き換え"

# 候補に宣言していないファイルが混ざった場合
rebuild
echo 'secret' >"$work_dir/bad/candidate/extra.txt"
reject "宣言していないファイルの混入"

# 候補と別のcommitを公開しようとした場合
rebuild
reject "source commitの不一致" "$other_commit"

# 候補を作ったあとに公開予定の内容が変わった場合
rebuild
echo '{"openapi":"3.0.0"}' >"$work_dir/bad/docs/openapi.json"
reject "作業treeとの不一致"

# 候補を作ったあとに版が変わった場合
rebuild
jq '.packageVersion = "1.2.4"' "$work_dir/bad/release-manifest.json" >"$work_dir/bad/manifest.new"
mv "$work_dir/bad/manifest.new" "$work_dir/bad/release-manifest.json"
reject "版の不一致"

# metadataそのものが書き換えられた場合
rebuild
jq '.assets[0].sha256 = "0000000000000000000000000000000000000000000000000000000000000000"' \
  "$work_dir/bad/candidate/release-candidate.json" >"$work_dir/bad/candidate/metadata.new"
mv "$work_dir/bad/candidate/metadata.new" "$work_dir/bad/candidate/release-candidate.json"
reject "metadataの書き換え"

# source commitの形式が不正な場合
rebuild
if bash "$script_dir/release-candidate.sh" verify "$work_dir/bad/candidate" "main" "$work_dir/bad" \
  >/dev/null 2>&1; then
  echo "受理してはいけない状態を受理しました: commit形式" >&2
  exit 1
fi

echo "release candidate検査の自己テストに成功しました。"
