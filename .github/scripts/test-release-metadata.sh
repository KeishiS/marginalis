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
    {name: "marginalis-domain", version: "0.36.0",
     license: "MIT OR Apache-2.0", publish: []},
    {name: "marginalis-service", version: "0.36.0",
     license: "MIT OR Apache-2.0", publish: []}
  ]}' >"$root/metadata.json"
}

build_tree "$work_dir/good"
bash "$script_dir/check-release-metadata.sh" \
  "$work_dir/good/metadata.json" "$work_dir/good" >/dev/null

reject() {
  local description=$1
  if bash "$script_dir/check-release-metadata.sh" \
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
jq '.packages[1].version = "0.37.0"' \
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

echo "リリースmetadata検査の自己テストに成功しました。"
