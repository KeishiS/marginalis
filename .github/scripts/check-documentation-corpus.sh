#!/usr/bin/env bash
set -euo pipefail

project_root="${1:-.}"
migration_manifest="${2:-.github/documentation-markdown-migration.txt}"
cd "$project_root"
if [[ ! -f "$migration_manifest" ]]; then
  echo "Markdown移行manifestがありません: $migration_manifest" >&2
  exit 1
fi

work_directory="$(mktemp -d)"
trap 'rm -rf "$work_directory"' EXIT
expected="$work_directory/expected.txt"
actual="$work_directory/actual.txt"

LC_ALL=C sort -u "$migration_manifest" >"$expected"
if ! diff -u "$migration_manifest" "$expected"; then
  echo "Markdown移行manifestは重複なしのbyte順で記述してください。" >&2
  exit 1
fi

git -c core.quotepath=false ls-files --cached --others --exclude-standard -- '*.md' |
  awk '$0 != "AGENTS.md"' |
  LC_ALL=C sort >"$actual"

if ! diff -u "$expected" "$actual"; then
  echo "AGENTS.md以外のMarkdownは移行manifestと一致させてください。" >&2
  exit 1
fi

while IFS= read -r source; do
  [[ -n "$source" ]] || continue
  if [[ ! -f "$source" ]]; then
    echo "Markdown移行manifestが存在しない文書を参照しています: $source" >&2
    exit 1
  fi
done <"$migration_manifest"

markdown_count="$(wc -l <"$actual")"
echo "文書形式の移行対象を確認しました: Markdown ${markdown_count}件"
