#!/usr/bin/env bash
set -euo pipefail

script_directory="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
work_directory="$(mktemp -d)"
trap 'rm -rf "$work_directory"' EXIT

git -C "$work_directory" init --quiet
git -C "$work_directory" config user.name "Marginalis CI"
git -C "$work_directory" config user.email "ci@example.invalid"
mkdir -p "$work_directory/docs"
printf '%s\n' '# 固定名' >"$work_directory/AGENTS.md"
git -C "$work_directory" add .

bash "$script_directory/check-documentation-corpus.sh" "$work_directory" >/dev/null

printf '%s\n' '# 未分類' >"$work_directory/docs/unclassified.md"
if bash "$script_directory/check-documentation-corpus.sh" "$work_directory" >/dev/null 2>&1; then
  echo "AGENTS.md以外のMarkdown文書を受理しました。" >&2
  exit 1
fi

rm "$work_directory/docs/unclassified.md"
git -C "$work_directory" add -u
bash "$script_directory/check-documentation-corpus.sh" "$work_directory" >/dev/null

echo "文書分類検査の回帰試験に成功しました。"
