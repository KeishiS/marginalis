#!/usr/bin/env bash
set -euo pipefail

status=0

if ! bash .github/scripts/check-documentation-corpus.sh; then
  status=1
fi
if ! bash .github/scripts/check-asciidoc.sh; then
  status=1
fi
if ! bash .github/scripts/check-release-instructions.sh; then
  status=1
fi

if [[ -d issues ]]; then
  echo "issues/は廃止されています。新しい作業項目はGitHub Issuesへ作成してください。" >&2
  status=1
fi

while IFS= read -r -d '' source; do
  [[ -f "$source" ]] || continue

  if grep -nE '[[:blank:]]+$' "$source"; then
    echo "trailing whitespace: $source" >&2
    status=1
  fi

  if ! bash .github/scripts/check-markdown-links.sh "$source"; then
    status=1
  fi
done < <(git ls-files --cached --others --exclude-standard -z -- '*.md' | sort -z)

exit "$status"
