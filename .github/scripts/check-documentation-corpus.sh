#!/usr/bin/env bash
set -euo pipefail

project_root="${1:-.}"
cd "$project_root"

# .claude/配下はエージェント設定の領域で、skill形式がSKILL.mdを要求するため除外します。
# release/notes.mdはGitHub ReleaseのNotesとしてそのまま公開する本文で、GitHubがMarkdownを
# 描画するため除外します。人間向け文書はAsciiDocで書く方針は変わりません。
unexpected=$(git -c core.quotepath=false ls-files --cached --others --exclude-standard -- '*.md' |
  awk '$0 != "AGENTS.md" && $0 != "release/notes.md" && $0 !~ /^\.claude\//')
if [[ -n "$unexpected" ]]; then
  printf '%s\n' "$unexpected" >&2
  echo "AGENTS.md、release/notes.md、.claude/配下以外のMarkdown文書は追加できません。" >&2
  exit 1
fi

echo "文書形式を確認しました: MarkdownはAGENTS.md、release/notes.md、.claude/配下だけです。"
