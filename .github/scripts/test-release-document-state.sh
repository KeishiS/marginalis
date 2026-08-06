#!/usr/bin/env bash
set -euo pipefail

script_directory="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
work_directory="$(mktemp -d)"
trap 'rm -rf "$work_directory"' EXIT

acceptance_file="$work_directory/acceptance.adoc"
changelog_file="$work_directory/CHANGELOG.adoc"
expected_version="1.2.3"

write_fixture() {
  local decision="$1"
  local changelog_status="$2"
  printf '* 公開判定: %s\n' "$decision" >"$acceptance_file"
  printf '== %s — %s\n' "$expected_version" "$changelog_status" >"$changelog_file"
}

expect_success() {
  local name="$1"
  local decision="$2"
  local changelog_status="$3"
  local release_tag="${4:-}"
  write_fixture "$decision" "$changelog_status"
  if ! MARGINALIS_RELEASE_TAG="$release_tag" bash \
    "$script_directory/check-release-document-state.sh" \
    "$expected_version" "$acceptance_file" "$changelog_file" >/dev/null; then
    echo "成功すべき文書状態を拒否しました: $name" >&2
    exit 1
  fi
}

expect_failure() {
  local name="$1"
  local decision="$2"
  local changelog_status="$3"
  local release_tag="${4:-}"
  write_fixture "$decision" "$changelog_status"
  if MARGINALIS_RELEASE_TAG="$release_tag" bash \
    "$script_directory/check-release-document-state.sh" \
    "$expected_version" "$acceptance_file" "$changelog_file" >/dev/null 2>&1; then
    echo "失敗すべき文書状態を受理しました: $name" >&2
    exit 1
  fi
}

expect_success "未判定と未公開" "未判定" "未公開"
expect_success "公開停止と未公開" "公開停止" "未公開"
expect_success "公開可と公開日" "公開可" "2026-08-06"
expect_success "タグ付きの公開可と公開日" "公開可" "2026-08-06" "v$expected_version"

expect_failure "公開可と未公開" "公開可" "未公開"
expect_failure "タグとworkspace版の不一致" "公開可" "2026-08-06" "v9.9.9"
expect_failure "タグ付きの未判定" "未判定" "未公開" "v$expected_version"

write_fixture "公開可" "2026-08-06"
printf '* 公開判定: 公開可\n' >>"$acceptance_file"
if bash "$script_directory/check-release-document-state.sh" \
  "$expected_version" "$acceptance_file" "$changelog_file" >/dev/null 2>&1; then
  echo "重複した公開判定を受理しました。" >&2
  exit 1
fi

write_fixture "公開可" "2026-08-06"
printf '== %s — 2026-08-06\n' "$expected_version" >>"$changelog_file"
if bash "$script_directory/check-release-document-state.sh" \
  "$expected_version" "$acceptance_file" "$changelog_file" >/dev/null 2>&1; then
  echo "重複した変更履歴見出しを受理しました。" >&2
  exit 1
fi

printf '= 受入結果\n' >"$acceptance_file"
printf '== %s — 2026-08-06\n' "$expected_version" >"$changelog_file"
if bash "$script_directory/check-release-document-state.sh" \
  "$expected_version" "$acceptance_file" "$changelog_file" >/dev/null 2>&1; then
  echo "公開判定がない受入結果を受理しました。" >&2
  exit 1
fi

printf '* 公開判定: 公開可\n' >"$acceptance_file"
printf '= 変更履歴\n' >"$changelog_file"
if bash "$script_directory/check-release-document-state.sh" \
  "$expected_version" "$acceptance_file" "$changelog_file" >/dev/null 2>&1; then
  echo "対象バージョンの見出しがない変更履歴を受理しました。" >&2
  exit 1
fi

echo "リリース文書状態の回帰試験に成功しました。"
