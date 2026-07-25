#!/usr/bin/env bash
set -euo pipefail

status=0

while IFS= read -r source; do
  if grep -nE '[[:blank:]]+$' "$source"; then
    echo "trailing whitespace: $source" >&2
    status=1
  fi

  while IFS= read -r match; do
    target="${match#']('}"
    target="${target%')'}"
    target="${target%%'#'*}"
    case "$target" in
      *://* | mailto:* | "")
        continue
        ;;
    esac
    resolved="$(dirname "$source")/$target"
    if [[ ! -f "$resolved" ]]; then
      echo "broken Markdown link: $source -> $target" >&2
      status=1
    fi
  done < <(grep -oE '\]\([^)]*[.]md(#[^)]*)?\)' "$source" || true)
done < <(find docs issues -type f -name '*.md' -print | sort)

exit "$status"
