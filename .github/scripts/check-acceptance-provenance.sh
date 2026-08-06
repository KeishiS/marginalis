#!/usr/bin/env bash
set -euo pipefail

expected_version="${1:?expected version is required}"
acceptance_file="${2:?acceptance file is required}"

fail() {
  echo "$1" >&2
  exit 1
}

required_field() {
  local label="$1"
  local -a lines=()
  mapfile -t lines < <(grep -F "* $label: " "$acceptance_file" || true)
  if [[ "${#lines[@]}" -ne 1 ]]; then
    fail "受入結果の項目が一意ではありません: $label: $acceptance_file"
  fi
  local value="${lines[0]#* "$label: "}"
  if [[ "$value" != '``'*'``' ]]; then
    fail "受入結果の項目は二重のバッククォートで囲んでください: $label: $acceptance_file"
  fi
  value="${value#\`\`}"
  value="${value%\`\`}"
  printf '%s\n' "$value"
}

optional_field() {
  local label="$1"
  local -a lines=()
  mapfile -t lines < <(grep -F "* $label: " "$acceptance_file" || true)
  if [[ "${#lines[@]}" -gt 1 ]]; then
    fail "受入結果の項目が重複しています: $label: $acceptance_file"
  fi
  if [[ "${#lines[@]}" -eq 0 ]]; then
    return
  fi
  local value="${lines[0]#* "$label: "}"
  if [[ "$value" != '``'*'``' ]]; then
    fail "受入結果の項目は二重のバッククォートで囲んでください: $label: $acceptance_file"
  fi
  value="${value#\`\`}"
  value="${value%\`\`}"
  printf '%s\n' "$value"
}

sha_pattern='^[0-9a-f]{40}$'
human_commit="$(required_field '人手受入対象コミット')"
human_tree="$(required_field '人手受入対象tree')"
release_target="$(required_field '最終リリース対象')"
recorded_release_commit="$(optional_field '最終リリースコミット')"
recorded_release_tree="$(optional_field '最終リリースtree')"

[[ "$human_commit" =~ $sha_pattern ]] ||
  fail "人手受入対象コミットは40桁の完全なSHAでなければなりません: $acceptance_file"
[[ "$human_tree" =~ $sha_pattern ]] ||
  fail "人手受入対象treeは40桁の完全なSHAでなければなりません: $acceptance_file"

expected_target="refs/tags/v${expected_version}^{commit}"
if [[ "$release_target" != "$expected_target" ]]; then
  fail "最終リリース対象は版に対応するタグのcommit参照でなければなりません: $expected_target"
fi

if [[ -n "$recorded_release_commit" || -n "$recorded_release_tree" ]]; then
  if [[ ! "$recorded_release_commit" =~ $sha_pattern || ! "$recorded_release_tree" =~ $sha_pattern ]]; then
    fail "最終リリースコミットとtreeは、両方を40桁の完全なSHAで記録してください: $acceptance_file"
  fi
fi
