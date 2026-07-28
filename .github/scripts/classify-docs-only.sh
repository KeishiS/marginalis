#!/usr/bin/env bash
set -euo pipefail

changed=false
while IFS= read -r -d '' path; do
  changed=true
  case "$path" in
    docs/openapi.json)
      printf '%s\n' false
      exit 0
      ;;
    docs/* | README.md | CONTRIBUTING.md | AGENTS.md | CHANGELOG.md | .github/ISSUE_TEMPLATE/*)
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
