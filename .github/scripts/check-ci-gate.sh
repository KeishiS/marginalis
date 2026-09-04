#!/usr/bin/env bash
set -euo pipefail

event_name="${1:?event name is required}"
docs_only="${2:?docs-only result is required}"
core_result="${3:?verify-core result is required}"
docs_result="${4:?verify-docs result is required}"
native_result="${5:?native-aarch64 result is required}"
browser_result="${6:?browser-smoke result is required}"
dependency_result="${7:?dependency-review result is required}"

case "$event_name" in
  pull_request)
    test "$dependency_result" = success
    ;;
  push)
    test "$dependency_result" = skipped
    ;;
  *)
    echo "未対応のCI eventです: $event_name" >&2
    exit 1
    ;;
esac

if [[ "$docs_only" == true ]]; then
  test "$core_result" = skipped
  test "$docs_result" = success
  test "$native_result" = skipped
  test "$browser_result" = skipped
else
  test "$core_result" = success
  test "$docs_result" = skipped
  test "$native_result" = success
  test "$browser_result" = success
fi
