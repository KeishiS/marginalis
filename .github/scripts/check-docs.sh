#!/usr/bin/env bash
set -euo pipefail

status=0

if ! bash .github/scripts/check-documentation-corpus.sh; then
  status=1
fi
if ! bash .github/scripts/check-asciidoc.sh; then
  status=1
fi

if [[ -d issues ]]; then
  echo "issues/は廃止されています。新しい作業項目はGitHub Issuesへ作成してください。" >&2
  status=1
fi

legacy_manifest=.github/legacy-issues-v0.5.0.txt
migration_map=docs/issue-migration.adoc
if [[ ! -f "$legacy_manifest" || ! -f "$migration_map" ]]; then
  echo "旧Issueの移行manifestまたは対応表がありません。" >&2
  status=1
else
  legacy_upstream_url='https://github.com/KeishiS/marginalis/blob/v0.5.0/issues/upstream'
  if ! grep -Fxq ":legacy-upstream: $legacy_upstream_url" "$migration_map"; then
    echo "旧上流提案のURL属性が正しくありません。" >&2
    status=1
  fi
  legacy_count=0
  while IFS= read -r legacy_path; do
    [[ -n "$legacy_path" ]] || continue
    legacy_count=$((legacy_count + 1))
    if [[ "$legacy_path" == issues/upstream/* ]]; then
      source_reference="{legacy-upstream}/${legacy_path#issues/upstream/}"
    else
      source_reference="https://github.com/KeishiS/marginalis/blob/v0.5.0/$legacy_path"
    fi
    occurrences=$(grep -Foc "$source_reference" "$migration_map" || true)
    if [[ "$occurrences" -ne 1 ]]; then
      echo "旧Issueの移行先が一意ではありません: $legacy_path ($occurrences)" >&2
      status=1
    fi
  done <"$legacy_manifest"
  if [[ "$legacy_count" -ne 59 ]]; then
    echo "旧Issue manifestは59件でなければなりません: $legacy_count" >&2
    status=1
  fi
  if ! git rev-parse --verify --quiet 'refs/tags/v0.5.0^{commit}' >/dev/null; then
    echo "旧Issue manifestの検証にはv0.5.0タグが必要です。" >&2
    status=1
  elif ! diff -u \
    <(git ls-tree -r --name-only v0.5.0 -- issues | sort) \
    <(sort "$legacy_manifest"); then
    echo "旧Issue manifestがv0.5.0タグのissues/と一致しません。" >&2
    status=1
  fi
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
