#!/usr/bin/env bash
set -euo pipefail

project_root="${1:-.}"
cd "$project_root"

unexpected=$(git -c core.quotepath=false ls-files --cached --others --exclude-standard -- '*.md' |
  awk '$0 != "AGENTS.md"')
if [[ -n "$unexpected" ]]; then
  printf '%s\n' "$unexpected" >&2
  echo "AGENTS.md以外のMarkdown文書は追加できません。" >&2
  exit 1
fi

echo "文書形式を確認しました: MarkdownはAGENTS.mdだけです。"
