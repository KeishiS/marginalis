#!/usr/bin/env bash
set -euo pipefail

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
work_dir=$(mktemp -d)
trap 'rm -rf "$work_dir"' EXIT

write_guide() {
  printf '%s\n' "$@" >"$work_dir/release.adoc"
}

write_guide \
  'gh release create v<MAJOR>.<MINOR>.<PATCH> --draft docs/openapi.json docs/mcp-tools.json \' \
  '  --title v<MAJOR>.<MINOR>.<PATCH> --notes-file <Release Notesのファイル>' \
  'gh release view v<MAJOR>.<MINOR>.<PATCH> --json isDraft,assets \' \
  "  --jq '{isDraft, assets: [.assets[].name]}'" \
  'gh release edit v<MAJOR>.<MINOR>.<PATCH> --draft=false'
bash "$script_dir/check-release-instructions.sh" "$work_dir/release.adoc" >/dev/null

reject() {
  local description=$1
  if bash "$script_dir/check-release-instructions.sh" "$work_dir/release.adoc" \
    >/dev/null 2>&1; then
    echo "受理してはいけないリリース手順を受理しました: $description" >&2
    exit 1
  fi
}

write_guide \
  'gh release create v<MAJOR>.<MINOR>.<PATCH> --draft docs/mcp-tools.json' \
  'gh release view v<MAJOR>.<MINOR>.<PATCH> --json isDraft,assets \' \
  'gh release edit v<MAJOR>.<MINOR>.<PATCH> --draft=false'
reject "OpenAPI assetの欠落"

write_guide \
  'gh release create v<MAJOR>.<MINOR>.<PATCH> --draft docs/openapi.json' \
  'gh release view v<MAJOR>.<MINOR>.<PATCH> --json isDraft,assets \' \
  'gh release edit v<MAJOR>.<MINOR>.<PATCH> --draft=false'
reject "MCP tool契約assetの欠落"

write_guide \
  'gh release create v<MAJOR>.<MINOR>.<PATCH> docs/openapi.json docs/mcp-tools.json' \
  'gh release view v<MAJOR>.<MINOR>.<PATCH> --json isDraft,assets \' \
  'gh release edit v<MAJOR>.<MINOR>.<PATCH> --draft=false'
reject "下書き指定の欠落"

write_guide \
  'gh release create v<MAJOR>.<MINOR>.<PATCH> --draft docs/openapi.json docs/mcp-tools.json' \
  'gh release edit v<MAJOR>.<MINOR>.<PATCH> --draft=false'
reject "公開前確認の欠落"

write_guide \
  'gh release create v<MAJOR>.<MINOR>.<PATCH> --draft docs/openapi.json docs/mcp-tools.json' \
  'gh release view v<MAJOR>.<MINOR>.<PATCH> --json isDraft,assets \'
reject "明示的な公開操作の欠落"

echo "リリース手順検査の自己テストに成功しました。"
