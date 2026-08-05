#!/usr/bin/env bash
set -euo pipefail

script="$(dirname "$0")/classify-docs-only.sh"

assert_classification() {
  expected="$1"
  shift
  if [ "$#" -eq 0 ]; then
    # 変更pathが無い場合を、呼び出し元の標準入力に依存せず再現する。
    # 明示しないと、対話的な端末など標準入力がEOFにならない環境で待ち続ける。
    actual="$(bash "$script" </dev/null)"
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
  README.adoc \
  CONTRIBUTING.adoc \
  SECURITY.adoc \
  docs/architecture.adoc \
  .github/ISSUE_TEMPLATE/feature.yml
assert_classification false docs/architecture.adoc frontend/src/Application.tsx
assert_classification false docs/openapi.json
assert_classification false docs/mcp-tools.json
assert_classification false CHANGELOG.adoc
assert_classification false docs/acceptance.adoc
assert_classification false docs/acceptance-results/v0.24.0.adoc
assert_classification true $'docs/architecture.adoc\nfrontend/src/Application.tsx'
