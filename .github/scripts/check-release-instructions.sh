#!/usr/bin/env bash
set -euo pipefail

release_guide="${1:-docs/developer-guide/release.adoc}"

fail() {
  echo "$1" >&2
  exit 1
}

declare -A instruction_lines
require_line() {
  local name=$1
  local instruction=$2
  local matches
  mapfile -t matches < <(grep -nFx -- "$instruction" "$release_guide" || true)
  if [[ "${#matches[@]}" -ne 1 ]]; then
    fail "リリース手順に必要な操作が一つだけ記載されていません: $name"
  fi
  instruction_lines["$name"]="${matches[0]%%:*}"
}

require_line tag 'release_tag=v<MAJOR>.<MINOR>.<PATCH>'
require_line temporary_directory 'release_assets=$(mktemp -d)'
require_line cleanup 'trap '\''rm -rf "$release_assets"'\'' EXIT'
require_line openapi 'git show "$release_tag:docs/openapi.json" >"$release_assets/openapi.json"'
require_line mcp_tools 'git show "$release_tag:docs/mcp-tools.json" >"$release_assets/mcp-tools.json"'
require_line create 'gh release create "$release_tag" --verify-tag --draft \'
require_line assets '  "$release_assets/openapi.json" "$release_assets/mcp-tools.json" \'
require_line release_metadata '  --title "$release_tag" --notes-file <Release Notesのファイル>'
require_line view 'test "$(gh release view "$release_tag" --json isDraft,assets \'
require_line assertion \
  '  --jq '\''(.isDraft == true) and (([.assets[].name] | sort) == ["mcp-tools.json", "openapi.json"])'\'')" = true'
require_line publish 'gh release edit "$release_tag" --draft=false'

ordered_steps=(
  tag temporary_directory cleanup openapi mcp_tools create assets release_metadata view assertion publish
)
previous_line=0
for step in "${ordered_steps[@]}"; do
  current_line=${instruction_lines[$step]}
  if ((current_line <= previous_line)); then
    fail "GitHub Releaseの操作順序が正しくありません: $step"
  fi
  previous_line=$current_line
done

echo "対象タグの公開契約抽出、Releaseの下書き作成、厳密な確認、明示的な公開手順を確認しました。"
