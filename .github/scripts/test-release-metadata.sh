#!/usr/bin/env bash
set -euo pipefail

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
work_dir=$(mktemp -d)
trap 'rm -rf "$work_dir"' EXIT

build_tree() {
  local root=$1
  mkdir -p "$root"
  echo 'MIT License' >"$root/LICENSE-MIT"
  echo 'Apache License' >"$root/LICENSE-APACHE"
  jq -n '{packages: [
    {name: "marginalis-domain", version: "1.2.3",
     license: "MIT OR Apache-2.0", publish: []},
    {name: "marginalis-service", version: "1.2.3",
     license: "MIT OR Apache-2.0", publish: []}
  ]}' >"$root/metadata.json"
}

build_tree "$work_dir/good"
RELEASE_TAG="v1.2.3" bash "$script_dir/check-release-metadata.sh" \
  "$work_dir/good/metadata.json" "$work_dir/good" >/dev/null

reject() {
  local description=$1
  if RELEASE_TAG="${2:-}" bash "$script_dir/check-release-metadata.sh" \
    "$work_dir/bad/metadata.json" "$work_dir/bad" >/dev/null 2>&1; then
    echo "受理してはいけない状態を受理しました: $description" >&2
    exit 1
  fi
}

rebuild() {
  rm -rf "$work_dir/bad"
  build_tree "$work_dir/bad"
}

# crateごとに版が食い違う場合
rebuild
jq '.packages[1].version = "1.2.4"' \
  "$work_dir/bad/metadata.json" >"$work_dir/bad/metadata.new"
mv "$work_dir/bad/metadata.new" "$work_dir/bad/metadata.json"
reject "版の不一致"

# dual license以外の宣言が混ざった場合
rebuild
jq '.packages[0].license = "GPL-3.0"' \
  "$work_dir/bad/metadata.json" >"$work_dir/bad/metadata.new"
mv "$work_dir/bad/metadata.new" "$work_dir/bad/metadata.json"
reject "想定外のライセンス"

# publish禁止が外れた場合
rebuild
jq 'del(.packages[0].publish)' \
  "$work_dir/bad/metadata.json" >"$work_dir/bad/metadata.new"
mv "$work_dir/bad/metadata.new" "$work_dir/bad/metadata.json"
reject "publish禁止の欠落"

# 宣言済みライセンス文書が欠けた場合
rebuild
rm "$work_dir/bad/LICENSE-APACHE"
reject "ライセンス文書の欠落"

# vから始まる通常版の形式でない場合
rebuild
reject "vがないリリースタグ" "1.2.3"

rebuild
reject "要素が不足したリリースタグ" "v1.2"

rebuild
reject "先頭に不要な0があるリリースタグ" "v01.2.3"

# 形式が正しくてもworkspaceの版と違う場合
rebuild
reject "workspaceの版と異なるリリースタグ" "v1.2.4"

echo "リリースmetadata検査の自己テストに成功しました。"
