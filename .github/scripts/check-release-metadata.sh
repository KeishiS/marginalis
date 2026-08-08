#!/usr/bin/env bash
set -euo pipefail

# リリース対象の版とライセンス宣言の一貫性を確かめます。workspaceの全crateが同じ版、
# 同じdual license、publish禁止であること、宣言済みのライセンス文書が存在すること、
# Nix packageが同じ版を報告することを検査します。

temporary_directory=$(mktemp -d)
trap 'rm -rf "$temporary_directory"' EXIT

metadata="$temporary_directory/metadata.json"
metadata_input="${1:-}"
project_root="${2:-.}"
release_tag="${3:-${RELEASE_TAG:-}}"

if [[ -n "$metadata_input" ]]; then
  cp "$metadata_input" "$metadata"
else
  cargo metadata --locked --no-deps --format-version 1 >"$metadata"
fi

cd "$project_root"

fail() {
  echo "$1" >&2
  exit 1
}

expected_version="$(
  jq -er '[.packages[].version] | unique
    | if length == 1 then .[0] else error("workspace crateの版が揃っていません") end' \
    "$metadata"
)" || fail "workspace crateの版が揃っていません。"

if [[ -n "$release_tag" ]]; then
  if [[ ! "$release_tag" =~ ^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]]; then
    fail "リリースタグはv<MAJOR>.<MINOR>.<PATCH>形式で指定してください: ${release_tag}"
  fi
  expected_tag="v${expected_version}"
  if [[ "$release_tag" != "$expected_tag" ]]; then
    fail "リリースタグがworkspaceの版と一致しません: expected=${expected_tag}, actual=${release_tag}"
  fi
fi

test -s LICENSE-MIT || fail "LICENSE-MITがありません。"
test -s LICENSE-APACHE || fail "LICENSE-APACHEがありません。"

jq -e --arg version "$expected_version" '
  all(.packages[];
    .version == $version
    and .license == "MIT OR Apache-2.0"
    and .publish == [])
' "$metadata" >/dev/null ||
  fail "版、ライセンス、またはpublish禁止の宣言が揃っていないcrateがあります。"

# fixtureでの自己テストではNixの評価を省略し、宣言の一貫性だけを検査します。
if [[ -n "$metadata_input" ]]; then
  echo "リリースmetadataの一貫性を確認しました: v${expected_version}"
  exit 0
fi

test "$(
  nix eval --raw .#packages.x86_64-linux.default.version
)" = "$expected_version" || fail "Nix packageの版がworkspaceの版と一致しません。"
test "$(
  nix eval --raw .#packages.x86_64-linux.frontend.version
)" = "$expected_version" || fail "frontend packageの版がworkspaceの版と一致しません。"

echo "リリースmetadataの一貫性を確認しました: v${expected_version}"
