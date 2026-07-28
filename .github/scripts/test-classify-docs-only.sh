#!/usr/bin/env bash
set -euo pipefail

script="$(dirname "$0")/classify-docs-only.sh"

assert_classification() {
  expected="$1"
  shift
  if [ "$#" -eq 0 ]; then
    actual="$(bash "$script")"
  else
    actual="$(printf '%s\0' "$@" | bash "$script")"
  fi
  if [ "$actual" != "$expected" ]; then
    printf '文書のみの変更判定が一致しません。期待値=%s 実際=%s\n' \
      "$expected" "$actual" >&2
    exit 1
  fi
}

assert_classification false
assert_classification true \
  README.md \
  docs/architecture.md \
  .github/ISSUE_TEMPLATE/feature.yml
assert_classification false docs/architecture.md frontend/src/Application.tsx
assert_classification false docs/openapi.json
assert_classification true $'docs/architecture.md\nfrontend/src/Application.tsx'
