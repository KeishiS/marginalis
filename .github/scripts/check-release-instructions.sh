#!/usr/bin/env bash
set -euo pipefail

# リリース操作の要点が失われていないことを検査します。
#   - 下書きReleaseの作成と契約assetの添付は、release-gate.ymlのdraft-releaseジョブが
#     タグの内容から自動で行います(--verify-tagと下書き・asset集合の厳密な確認を含む)。
#   - リリース手順の文書には、Release Notesの記載、下書きの確認、明示的な公開が
#     この順序で残っていなければなりません。

release_guide="${1:-docs/developer-guide/release.adoc}"
release_workflow="${2:-.github/workflows/release-gate.yml}"

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

# 行頭の字下げを無視して、workflow内の必須の操作を照合します。
require_workflow_line() {
  local name=$1
  local instruction=$2
  local count
  count=$(sed 's/^[[:space:]]*//' "$release_workflow" | grep -cFx -- "$instruction" || true)
  if [[ "$count" -ne 1 ]]; then
    fail "release workflowに必要な操作が一つだけ記載されていません: $name"
  fi
}

require_workflow_line create 'gh release create "$RELEASE_TAG" --verify-tag --draft \'
require_workflow_line assets 'docs/openapi.json docs/mcp-tools.json \'
require_workflow_line view 'test "$(gh release view "$RELEASE_TAG" --json isDraft,assets \'
require_workflow_line assertion \
  '--jq '\''(.isDraft == true) and (([.assets[].name] | sort) == ["mcp-tools.json", "openapi.json"])'\'')" = true'

require_line tag 'release_tag=v<MAJOR>.<MINOR>.<PATCH>'
require_line notes 'gh release edit "$release_tag" --notes-file <Release Notesのファイル>'
require_line view 'test "$(gh release view "$release_tag" --json isDraft,assets \'
require_line assertion \
  '  --jq '\''(.isDraft == true) and (([.assets[].name] | sort) == ["mcp-tools.json", "openapi.json"])'\'')" = true'
require_line publish 'gh release edit "$release_tag" --draft=false'

ordered_steps=(
  tag notes view assertion publish
)
previous_line=0
for step in "${ordered_steps[@]}"; do
  current_line=${instruction_lines[$step]}
  if ((current_line <= previous_line)); then
    fail "GitHub Releaseの操作順序が正しくありません: $step"
  fi
  previous_line=$current_line
done

echo "自動下書きの作成・厳密な確認と、Notes記載・確認・明示的な公開の手順を確認しました。"
