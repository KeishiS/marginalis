#!/usr/bin/env bash
set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
gate="$script_dir/check-ci-gate.sh"

expect_success() {
  bash "$gate" "$@"
}

expect_failure() {
  if bash "$gate" "$@" >/dev/null 2>&1; then
    echo "CI gateが誤って成功しました: $*" >&2
    exit 1
  fi
}

expect_success pull_request false success skipped success success success
expect_success pull_request true skipped success skipped skipped success
expect_success push false success skipped success success skipped
expect_success push true skipped success skipped skipped skipped

expect_failure pull_request false success skipped success success skipped
expect_failure pull_request false success skipped skipped success success
expect_failure pull_request true success success skipped skipped success
expect_failure push false success skipped success success success
expect_failure workflow_dispatch false success skipped success success skipped

echo "CI gateの分岐を検査しました。"
