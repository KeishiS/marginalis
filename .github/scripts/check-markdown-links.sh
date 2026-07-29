#!/usr/bin/env bash
set -euo pipefail

status=0

for source in "$@"; do
  while IFS= read -r match; do
    target="${match#']('}"
    target="${target%')'}"
    if [[ "$target" == '<'*'>' ]]; then
      target="${target#'<'}"
      target="${target%'>'}"
    fi
    target="${target%%'#'*}"
    target="${target%%'?'*}"
    case "$target" in
      "" | //* | /* | [a-zA-Z][a-zA-Z0-9+.-]*:*)
        continue
        ;;
    esac
    resolved="$(dirname "$source")/$target"
    if [[ ! -e "$resolved" ]]; then
      echo "Markdownのリンク先がありません: $source -> $target" >&2
      status=1
    fi
  done < <(grep -oE '\]\([^)]*\)' "$source" || true)
done

exit "$status"
