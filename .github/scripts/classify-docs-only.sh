#!/usr/bin/env bash
set -euo pipefail

changed=false
while IFS= read -r -d '' path; do
  changed=true
  case "$path" in
    CHANGELOG.md | docs/acceptance.md | docs/acceptance-results/* | docs/openapi.json | docs/mcp-tools.json)
      printf '%s\n' false
      exit 0
      ;;
    docs/* | README.md | CONTRIBUTING.md | AGENTS.md | .github/ISSUE_TEMPLATE/*)
      ;;
    *)
      printf '%s\n' false
      exit 0
      ;;
  esac
done

if [ "$changed" = true ]; then
  printf '%s\n' true
else
  printf '%s\n' false
fi
