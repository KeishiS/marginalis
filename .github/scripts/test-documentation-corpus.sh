#!/usr/bin/env bash
set -euo pipefail

script_dir="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
work_dir="$(mktemp -d)"
trap 'rm -rf "$work_dir"' EXIT

git -C "$work_dir" init --quiet
git -C "$work_dir" config user.name "Marginalis CI"
git -C "$work_dir" config user.email "ci@example.invalid"
mkdir -p "$work_dir/docs"
printf '%s\n' '# 固定名' >"$work_dir/AGENTS.md"
git -C "$work_dir" add .

bash "$script_dir/check-documentation-corpus.sh" "$work_dir" >/dev/null

printf '%s\n' '# 未分類' >"$work_dir/docs/unclassified.md"
if bash "$script_dir/check-documentation-corpus.sh" "$work_dir" >/dev/null 2>&1; then
  echo "AGENTS.md以外のMarkdown文書を受理しました。" >&2
  exit 1
fi

rm "$work_dir/docs/unclassified.md"
git -C "$work_dir" add -u
bash "$script_dir/check-documentation-corpus.sh" "$work_dir" >/dev/null

# .claude/配下のMarkdown(skill定義など)は受理する。
mkdir -p "$work_dir/.claude/skills/example"
printf '%s\n' '# skill' >"$work_dir/.claude/skills/example/SKILL.md"
git -C "$work_dir" add .
bash "$script_dir/check-documentation-corpus.sh" "$work_dir" >/dev/null

# GitHub Releaseへそのまま公開するrelease/notes.mdは受理する。
mkdir -p "$work_dir/release"
printf '%s\n' '# Marginalis v1.2.3' >"$work_dir/release/notes.md"
git -C "$work_dir" add .
bash "$script_dir/check-documentation-corpus.sh" "$work_dir" >/dev/null

echo "文書分類検査の回帰試験に成功しました。"
